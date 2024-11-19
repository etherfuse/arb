use crate::args::{JupiterQuoteArgs, RunArgs};
use crate::constants::{MIN_USDC_AMOUNT, STABLEBOND_DECIMALS, USDC_DECIMALS, USDC_MINT};
use crate::jupiter::Quote;
use crate::math;
use crate::{Arber, InstantBondRedemptionArgs, PurchaseArgs};

use anyhow::Result;
use solana_sdk::{pubkey::Pubkey, signer::Signer, transaction::VersionedTransaction};
use spl_associated_token_account::get_associated_token_address;
use std::collections::VecDeque;
use std::str::FromStr;
use std::time::{Duration, Instant};

struct RateLimiter {
    requests: VecDeque<Instant>,
    window: Duration,
    max_requests: usize,
}

impl RateLimiter {
    fn new(window_secs: u64, max_requests: usize) -> Self {
        Self {
            requests: VecDeque::new(),
            window: Duration::from_secs(window_secs),
            max_requests,
        }
    }

    async fn wait_if_needed(&mut self) {
        let now = Instant::now();

        // Remove old requests outside the window
        while let Some(request_time) = self.requests.front() {
            if now.duration_since(*request_time) > self.window {
                self.requests.pop_front();
            } else {
                break;
            }
        }

        // If at capacity, wait until we can make another request
        if self.requests.len() >= self.max_requests {
            if let Some(oldest) = self.requests.front() {
                let wait_time = self.window - now.duration_since(*oldest);
                tokio::time::sleep(wait_time).await;
            }
        }

        self.requests.push_back(now);
    }
}

impl Arber {
    pub async fn run(&self, args: RunArgs) -> Result<()> {
        // run a task that checks arb every 1 minute
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            self.check_arb(args.clone()).await?;
        }
    }

    async fn check_arb(&self, args: RunArgs) -> Result<()> {
        let mut rate_limiter = RateLimiter::new(1, 10);

        let usdc_balance = self.update_usdc_balance().await?;

        let stablebond_price_to_usd = self.get_etherfuse_price(args.etherfuse_token).await?;

        let max_usdc_ui_amount_to_purchase = math::checked_float_mul(usdc_balance, 0.99)?;
        let mut usdc_token_amount =
            math::to_token_amount(max_usdc_ui_amount_to_purchase, USDC_DECIMALS)?;

        let stablebond_ui_amount =
            math::checked_float_div(max_usdc_ui_amount_to_purchase, stablebond_price_to_usd)?;
        let mut stablebond_token_amount =
            math::to_token_amount(stablebond_ui_amount, STABLEBOND_DECIMALS)?;
        // get jupiter price of token based on quoted amount of USDC in users wallet

        while usdc_token_amount > MIN_USDC_AMOUNT {
            rate_limiter.wait_if_needed().await;

            let (jup_sell_price, sell_quote) = self
                .sell_quote(args.clone(), stablebond_token_amount)
                .await?;

            let (jup_buy_price, buy_quote) = self
                .buy_quote(args.clone(), stablebond_token_amount)
                .await?;

            println!(
                "Current prices (USDC per token) - Jupiter: sell at {:.4}, buy at {:.4}, Etherfuse: {:.4}",
                jup_sell_price, jup_buy_price, stablebond_price_to_usd
            );

            // If Jupiter's selling price is higher than Etherfuse, buy on Etherfuse and sell on Jupiter
            if jup_sell_price > stablebond_price_to_usd {
                println!(
                    "Arb opportunity found: Buy on Etherfuse at {:.4} and sell on Jupiter at {:.4}",
                    stablebond_price_to_usd, jup_sell_price,
                );
                let purchase_args = PurchaseArgs {
                    amount: stablebond_token_amount,
                    mint: args.etherfuse_token,
                };
                println!("purchase args: {:?}", purchase_args);
                let purchase_tx = self.purchase_tx(purchase_args).await?;
                let swap_tx = self.jupiter_swap_tx(sell_quote).await?;
                let txs: &[VersionedTransaction] = &[purchase_tx, swap_tx];
                self.send_bundle(txs).await?;
                break;
            }
            // If Etherfuse's price is higher than Jupiter's buying price, buy on Jupiter and sell on Etherfuse
            else if jup_buy_price < stablebond_price_to_usd {
                println!(
                    "Arb opportunity found: Buy on Jupiter at {:.4} and sell on Etherfuse at {:.4}",
                    jup_buy_price, stablebond_price_to_usd
                );
                // let swap_tx = self.jupiter_swap_tx(buy_quote).await?;
                // let redemption_args = InstantBondRedemptionArgs {
                //     amount: stablebond_token_amount,
                //     mint: args.etherfuse_token,
                // };
                // println!("redeem args: {:?}", redemption_args);
                break;
            }

            // Reduce amount if no opportunity found
            println!("No opportunity at current amount, reducing by 20%");
            usdc_token_amount = (usdc_token_amount as f64 * 0.8) as u64;
            stablebond_token_amount = (stablebond_token_amount as f64 * 0.8) as u64;
        }

        Ok(())
    }

    async fn sell_quote(&self, args: RunArgs, amount: u64) -> Result<(f64, Quote)> {
        let jupiter_quote_args = JupiterQuoteArgs {
            input_mint: args.etherfuse_token,
            output_mint: Pubkey::from_str(USDC_MINT).unwrap(),
            amount,
            slippage_bps: Some(args.slippage_bps.unwrap_or(300)),
        };
        let quote = self.get_jupiter_quote(jupiter_quote_args).await?;
        // Convert amounts to decimal values before division
        let jup_price_usd_to_token: f64 = quote.in_amount as f64 / quote.out_amount as f64;
        let jup_price_token_to_usd: f64 = 1 as f64 / jup_price_usd_to_token;
        Ok((jup_price_token_to_usd, quote))
    }

    async fn buy_quote(&self, args: RunArgs, amount: u64) -> Result<(f64, Quote)> {
        let jupiter_quote_args = JupiterQuoteArgs {
            input_mint: Pubkey::from_str(USDC_MINT).unwrap(),
            output_mint: args.etherfuse_token,
            amount,
            slippage_bps: Some(args.slippage_bps.unwrap_or(300)),
        };
        let quote = self.get_jupiter_quote(jupiter_quote_args).await?;
        // Convert amounts to decimal values before division
        let jup_price_usd_to_token: f64 = quote.in_amount as f64 / quote.out_amount as f64;
        let jup_price_token_to_usd: f64 = 1 as f64 / jup_price_usd_to_token;
        Ok((jup_price_usd_to_token, quote))
    }

    async fn update_usdc_balance(&self) -> Result<f64> {
        let user_usdc_token_account = get_associated_token_address(
            &self.signer().pubkey(),
            &Pubkey::from_str(USDC_MINT).unwrap(),
        );
        let token_account = self
            .rpc_client
            .get_token_account(&user_usdc_token_account)
            .await
            .expect("unable to get usdc token account");

        if let Some(token_account) = token_account {
            let usdc_token_account_balance = token_account.token_amount.ui_amount.unwrap();
            let mut usdc_balance = self.usdc_balance.write().unwrap();
            *usdc_balance = usdc_token_account_balance;
            return Ok(usdc_token_account_balance);
        }
        return Ok(0.0);
    }
}

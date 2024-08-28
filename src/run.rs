use crate::args::{JupiterQuoteArgs, JupiterSwapArgs, RunArgs};
use crate::constants;
use crate::jupiter::Quote;
use crate::{Arber, PurchaseArgs};

use anyhow::Result;
use solana_sdk::{pubkey::Pubkey, signer::Signer, transaction::VersionedTransaction};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;

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
        let usdc_balance = self.update_usdc_balance().await?;
        // get etherfuse price of token
        let etherfuse_price_token_to_usd = self.get_etherfuse_price(args.etherfuse_token).await?;
        let max_etherfuse_ui_amount_to_purchase =
            (usdc_balance / etherfuse_price_token_to_usd) * 0.95;
        let mut etherfuse_token_amount_to_purchase = to_token_amount(
            max_etherfuse_ui_amount_to_purchase,
            constants::ETHERFUSE_TOKEN_DECIMALS,
        );

        // get jupiter price of token based on quoted amount of USDC in users wallet
        let (mut jup_price_token_to_usd, mut quote) = self
            .sell_quote(args.clone(), etherfuse_token_amount_to_purchase)
            .await?;

        while etherfuse_token_amount_to_purchase > 0
            && (jup_price_token_to_usd < etherfuse_price_token_to_usd)
        {
            // reduce the amount of tokens to purchase to see if arb exists on smaller trade
            etherfuse_token_amount_to_purchase /= 2;
            (jup_price_token_to_usd, quote) = self
                .sell_quote(args.clone(), etherfuse_token_amount_to_purchase)
                .await?;
        }
        if etherfuse_token_amount_to_purchase > 0 {
            println!("Arb opportunity: jupiter token price > etherfuse price honored. Purchase tokens from etherfuse and sell on jupiter");
            let purchase_args = PurchaseArgs {
                amount: quote.in_amount,
                mint: args.etherfuse_token,
            };
            println!("Purchase args: {:?}", purchase_args);
            let purchase_tx = self.purchase_tx(purchase_args).await?;
            let _swap_tx = self.jupiter_swap_tx(quote).await?;
            let txs: &[VersionedTransaction] = &[swap_tx];
            self.send_bundle(txs).await?;
        } else {
            println!("Arb opportunity: etherfuse price honored < jupiter token price. Purchase tokens from jupiter and sell on etherfuse");
        }
        Ok(())
    }

    async fn sell_quote(&self, args: RunArgs, amount: u64) -> Result<(f64, Quote)> {
        let jupiter_quote_args = JupiterQuoteArgs {
            input_mint: args.etherfuse_token,
            output_mint: Pubkey::from_str(constants::USDC_MINT).unwrap(),
            amount,
            slippage_bps: Some(args.slippage_bps.unwrap_or(300)),
        };
        let quote = self.get_jupiter_quote(jupiter_quote_args).await?;
        let jup_price_usd_to_token: f64 = quote.in_amount as f64 / quote.out_amount as f64;
        let jup_price_token_to_usd: f64 = 1 as f64 / jup_price_usd_to_token;
        Ok((jup_price_token_to_usd, quote))
    }

    async fn update_usdc_balance(&self) -> Result<f64> {
        let user_usdc_token_account = get_associated_token_address(
            &self.signer().pubkey(),
            &Pubkey::from_str(constants::USDC_MINT).unwrap(),
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

pub fn to_token_amount(amount: f64, decimals: u8) -> u64 {
    (amount * 10u64.pow(decimals as u32) as f64) as u64
}

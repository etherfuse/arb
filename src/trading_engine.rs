use anyhow::Result;
use clap::{arg, command, Parser, Subcommand};
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    signature::{read_keypair_file, Keypair},
};

use crate::args::{JupiterQuoteArgs, RunArgs};
use crate::constants::{MIN_USDC_AMOUNT, STABLEBOND_DECIMALS, USDC_DECIMALS, USDC_MINT};
use crate::jupiter::Quote;
use crate::math;
use crate::rate_limiter::RateLimiter;
use crate::traits::{TokenAmountExt, UiAmountExt};
use crate::{InstantBondRedemptionArgs, PurchaseArgs, TradingEngine};

use itertools::Itertools;
use solana_account_decoder::parse_token::token_amount_to_ui_amount;
use solana_sdk::{pubkey::Pubkey, signer::Signer, transaction::VersionedTransaction};
use spl_associated_token_account::{
    get_associated_token_address, get_associated_token_address_with_program_id,
};
use spl_token_2022::ID as SPL_TOKEN_2022_PROGRAM_ID;
use stablebond_sdk::find_bond_pda;
use std::str::FromStr;
use std::sync::Arc;

impl TradingEngine {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        keypair_filepath: Option<String>,
        etherfuse_url: Option<String>,
        jupiter_quote_url: Option<String>,
        jito_client: HttpClient,
        jito_tip: Arc<std::sync::RwLock<u64>>,
    ) -> Self {
        Self {
            rpc_client,
            keypair_filepath,
            etherfuse_url,
            jupiter_quote_url,
            jito_client,
            jito_tip,
        }
    }

    pub fn signer(&self) -> Keypair {
        match self.keypair_filepath.clone() {
            Some(filepath) => read_keypair_file(filepath.clone())
                .expect(format!("No keypair found at {}", filepath).as_str()),
            None => panic!("No keypair provided"),
        }
    }
}

impl TradingEngine {
    pub async fn run(&mut self, args: RunArgs) -> Result<()> {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60 * 2));
        loop {
            interval.tick().await;

            let etherfuse_price_per_token = self.get_etherfuse_price(args.etherfuse_token).await?;
            let stablebond_holdings_token_amount =
                self.get_spl_token_22_balance(args.etherfuse_token).await?;
            let usdc_holdings_token_amount = self
                .get_spl_token_balance(Pubkey::from_str(USDC_MINT).unwrap())
                .await?;
            let sell_liquidity_usdc_amount = self
                .get_sell_liquidity_usdc_amount(find_bond_pda(args.etherfuse_token).0)
                .await?;
            let rate_limiter = RateLimiter::new(10, 10);

            let best_strategy = Strategy::new(
                etherfuse_price_per_token,
                sell_liquidity_usdc_amount,
                stablebond_holdings_token_amount,
                usdc_holdings_token_amount,
                rate_limiter,
                Arc::new(self.clone()), //TODO: fix this shit
            )
            .run_strategies(args.clone())
            .await?
            .into_iter()
            .sorted_by(|a, b| b.profit.partial_cmp(&a.profit).unwrap())
            .next()
            .unwrap();

            println!("Best strategy found: {:?}", best_strategy);
        }
    }

    async fn sell_quote(&self, args: RunArgs, amount: u64) -> Result<(f64, Quote)> {
        let jupiter_quote_args = JupiterQuoteArgs {
            input_mint: args.etherfuse_token,
            output_mint: Pubkey::from_str(USDC_MINT).unwrap(),
            amount,
            slippage_bps: Some(args.slippage_bps.unwrap_or(300)),
        };
        let quote = self.get_jupiter_quote(jupiter_quote_args).await?;
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
        let jup_price_token_to_usd: f64 = quote.in_amount as f64 / quote.out_amount as f64;
        Ok((jup_price_token_to_usd, quote))
    }

    async fn get_spl_token_balance(&self, mint: Pubkey) -> Result<u64> {
        let user_token_account = get_associated_token_address(&self.signer().pubkey(), &mint);
        let token_account = self
            .rpc_client
            .get_token_account(&user_token_account)
            .await?;

        if let Some(token_account) = token_account {
            return Ok(math::to_token_amount(
                token_account.token_amount.ui_amount.unwrap(),
                token_account.token_amount.decimals,
            )
            .unwrap());
        }
        return Ok(0);
    }

    async fn get_spl_token_22_balance(&self, mint: Pubkey) -> Result<u64> {
        let user_token_account = get_associated_token_address_with_program_id(
            &self.signer().pubkey(),
            &mint,
            &SPL_TOKEN_2022_PROGRAM_ID,
        );
        let token_account = self
            .rpc_client
            .get_token_account(&user_token_account)
            .await?;

        if let Some(token_account) = token_account {
            return Ok(math::to_token_amount(
                token_account.token_amount.ui_amount.unwrap(),
                token_account.token_amount.decimals,
            )
            .unwrap());
        }
        return Ok(0);
    }

    async fn get_sell_liquidity_usdc_amount(&self, bond: Pubkey) -> Result<u64> {
        let sell_liquidity_usdc_amount = self.fetch_sell_liquidity_usdc_amount(bond).await?;
        Ok(sell_liquidity_usdc_amount)
    }
}

pub struct StrategyResult {
    pub profit: f64,
    pub txs: Vec<VersionedTransaction>,
}

impl std::fmt::Debug for StrategyResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Profit: {}", self.profit)
    }
}

pub struct Strategy {
    pub etherfuse_price_per_token: f64,
    pub sell_liquidity_usdc_amount: u64,
    pub stablebond_holdings_token_amount: u64,
    pub usdc_holdings_token_amount: u64,
    pub rate_limiter: RateLimiter,
    pub TradingEngine: Arc<TradingEngine>,
}

impl Strategy {
    pub fn new(
        etherfuse_price_per_token: f64,
        sell_liquidity_usdc_amount: u64,
        stablebond_holdings_token_amount: u64,
        usdc_holdings_token_amount: u64,
        rate_limiter: RateLimiter,
        TradingEngine: Arc<TradingEngine>,
    ) -> Self {
        Self {
            etherfuse_price_per_token,
            sell_liquidity_usdc_amount,
            stablebond_holdings_token_amount,
            usdc_holdings_token_amount,
            rate_limiter,
            TradingEngine,
        }
    }

    async fn run_strategies(&mut self, args: RunArgs) -> Result<Vec<StrategyResult>> {
        let mut results: Vec<StrategyResult> = Vec::new();
        results.push(
            self.check_jupiter_buy_etherfuse_sell_opportunity(args.clone())
                .await?,
        );
        results.push(
            self.check_jupiter_sell_etherfuse_buy_opportunity(args.clone())
                .await?,
        );
        Ok(results)
    }

    async fn check_jupiter_buy_etherfuse_sell_opportunity(
        &mut self,
        args: RunArgs,
    ) -> Result<StrategyResult> {
        if self.sell_liquidity_usdc_amount == 0 {
            return Ok(StrategyResult {
                profit: 0.0,
                txs: Vec::new(),
            });
        }
        let stablebond_holdings_in_usdc_ui_amount = math::checked_float_mul(
            self.stablebond_holdings_token_amount
                .to_ui_amount(STABLEBOND_DECIMALS),
            self.etherfuse_price_per_token,
        )?;

        let max_usdc_ui_amount_to_redeem = self
            .sell_liquidity_usdc_amount
            .to_ui_amount(USDC_DECIMALS)
            .min(stablebond_holdings_in_usdc_ui_amount);

        let max_usdc_token_amount_to_redeem =
            math::to_token_amount(max_usdc_ui_amount_to_redeem, USDC_DECIMALS)?;

        // let max_stablebond_ui_amount_to_redeem =
        //     math::checked_float_div(max_usdc_ui_amount_to_redeem, self.etherfuse_price_per_token)?;

        // let mut max_stablebond_token_amount_to_redeem =
        //     math::to_token_amount(max_stablebond_ui_amount_to_redeem, STABLEBOND_DECIMALS)?;

        let mut best_profit = 0.0;
        let mut best_usdc_amount = 0;
        let mut best_stablebond_amount = 0;
        let mut best_quote: Option<Quote> = None;

        let mut left = MIN_USDC_AMOUNT;
        let mut right = max_usdc_token_amount_to_redeem;

        while left <= right {
            let mid_usdc = left + (right - left) / 2;
            let mid_stablebond = (mid_usdc as f64 / self.etherfuse_price_per_token) as u64;

            self.rate_limiter.wait_if_needed().await;

            let (price_when_buying, buy_quote) = loop {
                match self.TradingEngine.buy_quote(args.clone(), mid_usdc).await {
                    Ok(quote) => break quote,
                    Err(e) => {
                        println!("Error getting buy quote, retrying: {}", e);
                        self.rate_limiter.wait_if_needed().await;
                    }
                }
            };

            let potential_profit = math::profit_from_arb(
                price_when_buying,
                self.etherfuse_price_per_token,
                mid_stablebond.to_ui_amount(STABLEBOND_DECIMALS),
            )?;

            if potential_profit > best_profit {
                best_profit = potential_profit;
                best_usdc_amount = mid_usdc;
                best_stablebond_amount = mid_stablebond;
                best_quote = Some(buy_quote);
                left = mid_usdc + 1;
            } else {
                right = mid_usdc - 1;
            }
        }
        println!(
            "Jupiter Buy -> Etherfuse Sell\nUSDC Amount: {}\nStablebond Amount: {}\nProfit: {}",
            token_amount_to_ui_amount(best_usdc_amount, USDC_DECIMALS).ui_amount_string,
            token_amount_to_ui_amount(best_stablebond_amount, STABLEBOND_DECIMALS).ui_amount_string,
            best_profit
        );
        let redemption_args = InstantBondRedemptionArgs {
            amount: best_stablebond_amount,
            mint: args.etherfuse_token,
        };
        println!("Redemption args: {:?}", redemption_args);
        let mut txs: Vec<VersionedTransaction> = Vec::new();
        if let Ok(update_oracle_tx) = self.TradingEngine.get_update_switchboard_oracle_tx().await {
            txs.push(update_oracle_tx);
        }
        if let Ok(buy_on_jupiter_tx) = self
            .TradingEngine
            .jupiter_swap_tx(best_quote.unwrap())
            .await
        {
            if let Ok(redeem_on_etherfuse_tx) = self
                .TradingEngine
                .instant_bond_redemption_tx(redemption_args)
                .await
            {
                txs.push(buy_on_jupiter_tx);
                txs.push(redeem_on_etherfuse_tx);
            }
        }
        return Ok(StrategyResult {
            profit: best_profit,
            txs,
        });
    }

    async fn check_jupiter_sell_etherfuse_buy_opportunity(
        &mut self,
        args: RunArgs,
    ) -> Result<StrategyResult> {
        let max_usdc_ui_amount_to_purchase = math::checked_float_mul(
            self.usdc_holdings_token_amount.to_ui_amount(USDC_DECIMALS),
            0.99,
        )?;
        let max_usdc_token_amount_to_purchase =
            max_usdc_ui_amount_to_purchase.to_token_amount(USDC_DECIMALS);

        // let stablebond_ui_amount_to_sell = math::checked_float_div(
        //     max_usdc_ui_amount_to_purchase,
        //     self.etherfuse_price_per_token,
        // )?;

        // let stablebond_token_amount_to_sell =
        //     stablebond_ui_amount_to_sell.to_token_amount(STABLEBOND_DECIMALS);

        let mut best_profit = 0.0;
        let mut best_usdc_amount = 0;
        let mut best_stablebond_amount = 0;
        let mut best_quote: Option<Quote> = None;

        let mut left = MIN_USDC_AMOUNT;
        let mut right = max_usdc_token_amount_to_purchase;

        while left <= right {
            let mid_usdc = left + (right - left) / 2;
            let mid_stablebond = (mid_usdc as f64 / self.etherfuse_price_per_token) as u64;

            self.rate_limiter.wait_if_needed().await;

            let (price_per_token_when_selling, sell_quote) = loop {
                match self
                    .TradingEngine
                    .sell_quote(args.clone(), mid_stablebond)
                    .await
                {
                    Ok(quote) => break quote,
                    Err(e) => {
                        println!("Error getting sell quote, retrying: {}", e);
                        self.rate_limiter.wait_if_needed().await;
                    }
                }
            };

            let potential_profit = math::profit_from_arb(
                price_per_token_when_selling,
                self.etherfuse_price_per_token,
                mid_stablebond.to_ui_amount(STABLEBOND_DECIMALS),
            )?;

            if potential_profit > best_profit {
                best_profit = potential_profit;
                best_usdc_amount = mid_usdc;
                best_stablebond_amount = mid_stablebond;
                best_quote = Some(sell_quote);
                left = mid_usdc + 1;
            } else {
                right = mid_usdc - 1;
            }
        }

        println!(
            "\nJupiter Sell -> Etherfuse Buy\nUSDC Amount: {}\nStablebond Amount: {}\nProfit: {}\n",
            token_amount_to_ui_amount(best_usdc_amount, USDC_DECIMALS).ui_amount_string,
            token_amount_to_ui_amount(best_stablebond_amount, STABLEBOND_DECIMALS).ui_amount_string,
            best_profit
        );
        let purchase_args = PurchaseArgs {
            amount: best_usdc_amount,
            mint: args.etherfuse_token,
        };
        println!("Purchase args: {:?}", purchase_args);
        let mut txs: Vec<VersionedTransaction> = Vec::new();
        if let Ok(update_oracle_tx) = self.TradingEngine.get_update_switchboard_oracle_tx().await {
            txs.push(update_oracle_tx);
        }
        if let Ok(buy_on_etherfuse_tx) = self.TradingEngine.purchase_tx(purchase_args).await {
            if let Ok(sell_on_jupiter_tx) = self
                .TradingEngine
                .jupiter_swap_tx(best_quote.unwrap())
                .await
            {
                txs.push(buy_on_etherfuse_tx);
                txs.push(sell_on_jupiter_tx);
            }
        }
        return Ok(StrategyResult {
            profit: best_profit,
            txs,
        });
    }
}

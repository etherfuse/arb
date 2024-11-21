use crate::market_data::MarketData;
use crate::math;
use crate::switchboard::SwitchboardClient;
use crate::traits::{TokenAmountExt, UiAmountExt};
use crate::{
    constants::{MIN_USDC_AMOUNT, STABLEBOND_DECIMALS, USDC_DECIMALS},
    jupiter::JupiterClient,
};
use crate::{etherfuse::EtherfuseClient, jupiter::Quote};
use crate::{InstantBondRedemptionArgs, PurchaseArgs};
use anyhow::Result;
use enum_dispatch::enum_dispatch;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, transaction::VersionedTransaction};
use std::sync::Arc;

#[enum_dispatch]
pub trait Strategy {
    async fn process_market_data(
        &mut self,
        md: &MarketData,
        stablebond_mint: &Pubkey,
    ) -> Result<StrategyResult>;
}

#[derive(Clone)]
pub struct BuyEtherfuseSellJupiter {
    pub rpc_client: Arc<RpcClient>,
    pub keypair_filepath: String,
    pub jupiter_client: JupiterClient,
    pub switchboard_client: SwitchboardClient,
    pub etherfuse_client: EtherfuseClient,
}

impl BuyEtherfuseSellJupiter {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        jupiter_client: JupiterClient,
        keypair_filepath: String,
        switchboard_client: SwitchboardClient,
        etherfuse_client: EtherfuseClient,
    ) -> Self {
        BuyEtherfuseSellJupiter {
            rpc_client,
            keypair_filepath,
            jupiter_client,
            switchboard_client,
            etherfuse_client,
        }
    }
}

#[derive(Clone)]
pub struct JupiterSellBuyEtherfuse {
    pub rpc_client: Arc<RpcClient>,
    pub jupiter_client: JupiterClient,
    pub keypair_filepath: String,
    pub switchboard_client: SwitchboardClient,
    pub etherfuse_client: EtherfuseClient,
}

impl JupiterSellBuyEtherfuse {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        jupiter_client: JupiterClient,
        keypair_filepath: String,
        switchboard_client: SwitchboardClient,
        etherfuse_client: EtherfuseClient,
    ) -> Self {
        JupiterSellBuyEtherfuse {
            rpc_client,
            jupiter_client,
            keypair_filepath,
            switchboard_client,
            etherfuse_client,
        }
    }
}

#[enum_dispatch(Strategy)]
pub enum StrategyEnum {
    BuyEtherfuseSellJupiter,
    JupiterSellBuyEtherfuse,
}

impl Strategy for JupiterSellBuyEtherfuse {
    async fn process_market_data(
        &mut self,
        md: &MarketData,
        stablebond_mint: &Pubkey,
    ) -> Result<StrategyResult> {
        let sell_liquidity_usdc_amount = md
            .sell_liquidity_usdc_amount
            .ok_or_else(|| anyhow::anyhow!("Missing sell_liquidity_usdc_amount"))?;
        let stablebond_holdings_token_amount = md
            .stablebond_holdings_token_amount
            .ok_or_else(|| anyhow::anyhow!("Missing stablebond_holdings_token_amount"))?;
        let etherfuse_price_per_token = md
            .etherfuse_price_per_token
            .ok_or_else(|| anyhow::anyhow!("Missing etherfuse_price_per_token"))?;

        if stablebond_holdings_token_amount == 0 {
            return Err(anyhow::anyhow!(
                "Existing stablebond holdings are required for this strategy"
            ));
        }
        if sell_liquidity_usdc_amount == 0 {
            return Err(anyhow::anyhow!(
                "Sell liquidity in USDC is required for this strategy"
            ));
        }
        let stablebond_holdings_in_usdc_ui_amount = math::checked_float_mul(
            stablebond_holdings_token_amount.to_ui_amount(STABLEBOND_DECIMALS),
            etherfuse_price_per_token,
        )?;

        let max_usdc_ui_amount_to_redeem = sell_liquidity_usdc_amount
            .to_ui_amount(USDC_DECIMALS)
            .min(stablebond_holdings_in_usdc_ui_amount);

        let max_usdc_token_amount_to_redeem =
            math::to_token_amount(max_usdc_ui_amount_to_redeem, USDC_DECIMALS)?;

        // let max_stablebond_ui_amount_to_redeem =
        //     math::checked_float_div(max_usdc_ui_amount_to_redeem, self.etherfuse_price_per_token)?;

        // let mut max_stablebond_token_amount_to_redeem =
        //     math::to_token_amount(max_stablebond_ui_amount_to_redeem, STABLEBOND_DECIMALS)?;

        let mut best_profit: f64 = 0.0;
        let mut best_usdc_amount = 0;
        let mut best_stablebond_amount = 0;
        let mut best_quote: Option<Quote> = None;

        // Strategy constants
        const MIN_TRADE_PERCENT: f64 = 0.01; // 1% of max amount
        const MAX_TRADE_PERCENT: f64 = 1.0; // 100% of max amount
        const INITIAL_POINTS: usize = 8; // Test 8 different sizes
        const MAX_RETRIES: u32 = 3;
        const RETRY_DELAY_MS: u64 = 60000;

        let max_amount = max_usdc_token_amount_to_redeem;

        // Generate initial test points with exponential distribution
        let points: Vec<f64> = (0..INITIAL_POINTS)
            .map(|i| {
                let t = i as f64 / (INITIAL_POINTS - 1) as f64;
                let exp_t = t.powf(1.5); // Exponential distribution
                MIN_TRADE_PERCENT + (MAX_TRADE_PERCENT - MIN_TRADE_PERCENT) * exp_t
            })
            .collect();

        println!("\n📊 Initial scan points (% of max amount):");
        for p in &points {
            println!("{:.1}%", p * 100.0);
        }

        // Test each trade size
        for trade_percent in points {
            let usdc_amount = (max_amount as f64 * trade_percent) as u64;
            let stablebond_amount = (usdc_amount as f64 / etherfuse_price_per_token) as u64;

            // Skip tiny amounts
            if usdc_amount < MIN_USDC_AMOUNT {
                continue;
            }

            // Get quote with retries
            let mut retries = 0;
            let quote_result = loop {
                match self
                    .jupiter_client
                    .buy_quote(stablebond_mint, usdc_amount)
                    .await
                {
                    Ok(quote) => break Some(quote),
                    Err(e) => {
                        retries += 1;
                        if retries >= MAX_RETRIES {
                            println!("Failed to get quote after {} retries: {}", MAX_RETRIES, e);
                            break None;
                        }
                        println!("Retry {}/{}: {}", retries, MAX_RETRIES, e);
                        tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS))
                            .await;
                    }
                }
            };

            let (price_when_buying, buy_quote) = match quote_result {
                Some(quote) => quote,
                None => continue,
            };

            // Calculate price impact
            let price_impact =
                (price_when_buying - etherfuse_price_per_token) / etherfuse_price_per_token;

            let potential_profit = match math::profit_from_arb(
                price_when_buying,
                etherfuse_price_per_token,
                stablebond_amount.to_ui_amount(STABLEBOND_DECIMALS),
            ) {
                Ok(profit) => profit,
                Err(e) => {
                    println!("Error calculating profit: {}. Skipping.", e);
                    continue;
                }
            };

            println!("\nTrade Analysis:");
            println!("Trade Size: {}% of max", trade_percent * 100.0);
            println!("USDC Amount: {}", usdc_amount);
            println!("Price Impact: {:.2}%", price_impact * 100.0);
            println!("Potential Profit: {}", potential_profit);
            println!("Buy Price: {}", price_when_buying);
            println!("Base Price: {}", etherfuse_price_per_token);

            if potential_profit > best_profit {
                println!("\n🎯 New best trade found!");
                println!("Previous best profit: {}", best_profit);
                println!("New best profit: {}", potential_profit);

                best_profit = potential_profit;
                best_usdc_amount = usdc_amount;
                best_stablebond_amount = stablebond_amount;
                best_quote = Some(buy_quote);
            }
        }

        if best_quote.is_none() {
            return Err(anyhow::anyhow!("No profitable trades found"));
        }

        println!("\n🏁 Search Complete");
        println!("Final best profit: {}", best_profit);
        println!("Final USDC amount: {}", best_usdc_amount);
        println!("Final Stablebond amount: {}", best_stablebond_amount);
        let redemption_args = InstantBondRedemptionArgs {
            amount: best_stablebond_amount,
            mint: stablebond_mint.clone(),
        };
        println!("Redemption args: {:?}", redemption_args);
        let mut txs: Vec<VersionedTransaction> = Vec::new();
        if let Ok(update_oracle_tx) = self
            .switchboard_client
            .get_update_switchboard_oracle_tx()
            .await
        {
            txs.push(update_oracle_tx);
        }
        if let Ok(buy_on_jupiter_tx) = self
            .jupiter_client
            .jupiter_swap_tx(best_quote.unwrap())
            .await
        {
            if let Ok(redeem_on_etherfuse_tx) = self
                .etherfuse_client
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
}

impl Strategy for BuyEtherfuseSellJupiter {
    async fn process_market_data(
        &mut self,
        md: &MarketData,
        stablebond_mint: &Pubkey,
    ) -> Result<StrategyResult> {
        let usdc_holdings_token_amount = md
            .usdc_holdings_token_amount
            .ok_or_else(|| anyhow::anyhow!("Missing usdc_holdings_token_amount"))?;
        let etherfuse_price_per_token = md
            .etherfuse_price_per_token
            .ok_or_else(|| anyhow::anyhow!("Missing etherfuse_price_per_token"))?;

        if usdc_holdings_token_amount == 0 {
            return Err(anyhow::anyhow!(
                "USDC holdings are required for this strategy"
            ));
        }

        let max_usdc_ui_amount_to_purchase =
            math::checked_float_mul(usdc_holdings_token_amount.to_ui_amount(USDC_DECIMALS), 0.99)?;
        let max_usdc_token_amount_to_purchase =
            max_usdc_ui_amount_to_purchase.to_token_amount(USDC_DECIMALS);

        // let stablebond_ui_amount_to_sell = math::checked_float_div(
        //     max_usdc_ui_amount_to_purchase,
        //     self.etherfuse_price_per_token,
        // )?;

        // let stablebond_token_amount_to_sell =
        //     stablebond_ui_amount_to_sell.to_token_amount(STABLEBOND_DECIMALS);

        let mut best_profit: f64 = 0.0;
        let mut best_usdc_amount = 0;
        let mut best_stablebond_amount = 0;
        let mut best_quote: Option<Quote> = None;

        // Strategy constants
        const MIN_TRADE_PERCENT: f64 = 0.01; // 1% of max amount
        const MAX_TRADE_PERCENT: f64 = 1.0; // 100% of max amount
        const INITIAL_POINTS: usize = 8; // Test 8 different sizes
        const MAX_RETRIES: u32 = 3;
        const RETRY_DELAY_MS: u64 = 60000; // Kept your 60s retry delay

        let max_amount = max_usdc_token_amount_to_purchase;

        // Generate initial test points with exponential distribution
        let points: Vec<f64> = (0..INITIAL_POINTS)
            .map(|i| {
                let t = i as f64 / (INITIAL_POINTS - 1) as f64;
                let exp_t = t.powf(1.5); // Exponential distribution
                MIN_TRADE_PERCENT + (MAX_TRADE_PERCENT - MIN_TRADE_PERCENT) * exp_t
            })
            .collect();

        println!("\n📊 Initial scan points (% of max amount):");
        for p in &points {
            println!("{:.1}%", p * 100.0);
        }

        // Test each trade size
        for trade_percent in points {
            let usdc_amount = (max_amount as f64 * trade_percent) as u64;
            let stablebond_amount = (usdc_amount as f64 / etherfuse_price_per_token) as u64;

            // Skip tiny amounts
            if usdc_amount < MIN_USDC_AMOUNT {
                continue;
            }

            // Get quote with retries
            let mut retries = 0;
            let quote_result = loop {
                match self
                    .jupiter_client
                    .sell_quote(stablebond_mint, stablebond_amount)
                    .await
                {
                    Ok(quote) => break Some(quote),
                    Err(e) => {
                        retries += 1;
                        if retries >= MAX_RETRIES {
                            println!("Failed to get quote after {} retries: {}", MAX_RETRIES, e);
                            break None;
                        }
                        println!("Retry {}/{}: {}", retries, MAX_RETRIES, e);
                        tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS))
                            .await;
                    }
                }
            };

            let (price_per_token_when_selling, sell_quote) = match quote_result {
                Some(quote) => quote,
                None => continue,
            };

            // Calculate price impact (note the reversed order for selling)
            let price_impact = (etherfuse_price_per_token - price_per_token_when_selling)
                / etherfuse_price_per_token;

            let potential_profit = match math::profit_from_arb(
                price_per_token_when_selling,
                etherfuse_price_per_token,
                stablebond_amount.to_ui_amount(STABLEBOND_DECIMALS),
            ) {
                Ok(profit) => profit,
                Err(e) => {
                    println!("Error calculating profit: {}. Skipping.", e);
                    continue;
                }
            };

            println!("\nTrade Analysis:");
            println!("Trade Size: {}% of max", trade_percent * 100.0);
            println!("USDC Amount: {}", usdc_amount);
            println!("Price Impact: {:.2}%", price_impact * 100.0);
            println!("Potential Profit: {}", potential_profit);
            println!("Sell Price: {}", price_per_token_when_selling);
            println!("Base Price: {}", etherfuse_price_per_token);

            if potential_profit > best_profit {
                println!("\n🎯 New best trade found!");
                println!("Previous best profit: {}", best_profit);
                println!("New best profit: {}", potential_profit);

                best_profit = potential_profit;
                best_usdc_amount = usdc_amount;
                best_stablebond_amount = stablebond_amount;
                best_quote = Some(sell_quote);
            }
        }

        if best_quote.is_none() {
            return Err(anyhow::anyhow!("No profitable trades found"));
        }

        println!("\n🏁 Search Complete");
        println!("Final best profit: {}", best_profit);
        println!("Final USDC amount: {}", best_usdc_amount);
        println!("Final Stablebond amount: {}", best_stablebond_amount);
        let purchase_args = PurchaseArgs {
            amount: best_usdc_amount,
            mint: stablebond_mint.clone(),
        };
        println!("Purchase args: {:?}\n", purchase_args);
        let mut txs: Vec<VersionedTransaction> = Vec::new();
        if let Ok(update_oracle_tx) = self
            .switchboard_client
            .get_update_switchboard_oracle_tx()
            .await
        {
            txs.push(update_oracle_tx);
        }
        if let Ok(buy_on_etherfuse_tx) = self.etherfuse_client.purchase_tx(purchase_args).await {
            if let Ok(sell_on_jupiter_tx) = self
                .jupiter_client
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

#[derive(Clone)]
pub struct StrategyResult {
    pub profit: f64,
    pub txs: Vec<VersionedTransaction>,
}

impl std::fmt::Debug for StrategyResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Profit: {}, Tx Count: {}", self.profit, self.txs.len())
    }
}

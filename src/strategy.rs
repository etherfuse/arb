use crate::market_data::MarketData;
use crate::math;
use crate::math::{TokenAmountExt, UiAmountExt};
use crate::{
    constants::{
        INITIAL_POINTS, MAX_RETRIES, MAX_TRADE_PERCENT, MAX_USDC_AMOUNT_PER_TRADE,
        MIN_TRADE_PERCENT, MIN_USDC_AMOUNT, RETRY_DELAY_MS, SLIPPAGE_BIPS, STABLEBOND_DECIMALS,
        USDC_DECIMALS,
    },
    jupiter::JupiterClient,
};
use crate::{etherfuse::EtherfuseClient, jupiter::Quote};
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
pub struct BuyOnEtherfuseSellOnJupiter {
    pub rpc_client: Arc<RpcClient>,
    pub keypair_filepath: String,
    pub jupiter_client: JupiterClient,
    pub etherfuse_client: EtherfuseClient,
}

impl BuyOnEtherfuseSellOnJupiter {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        jupiter_client: JupiterClient,
        keypair_filepath: String,
        etherfuse_client: EtherfuseClient,
    ) -> Self {
        BuyOnEtherfuseSellOnJupiter {
            rpc_client,
            keypair_filepath,
            jupiter_client,
            etherfuse_client,
        }
    }
}

#[derive(Clone)]
pub struct BuyOnJupiterSellOnEtherfuse {
    pub rpc_client: Arc<RpcClient>,
    pub jupiter_client: JupiterClient,
    pub keypair_filepath: String,
    pub etherfuse_client: EtherfuseClient,
}

impl BuyOnJupiterSellOnEtherfuse {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        jupiter_client: JupiterClient,
        keypair_filepath: String,
        etherfuse_client: EtherfuseClient,
    ) -> Self {
        BuyOnJupiterSellOnEtherfuse {
            rpc_client,
            jupiter_client,
            keypair_filepath,
            etherfuse_client,
        }
    }
}

#[derive(Clone)]
pub struct SellOnJupiterBuyOnEtherfuse {
    pub rpc_client: Arc<RpcClient>,
    pub jupiter_client: JupiterClient,
    pub keypair_filepath: String,
    pub etherfuse_client: EtherfuseClient,
}

#[enum_dispatch(Strategy)]
pub enum StrategyEnum {
    BuyOnJupiterSellOnEtherfuse,
    BuyOnEtherfuseSellOnJupiter,
}

impl Strategy for BuyOnJupiterSellOnEtherfuse {
    async fn process_market_data(
        &mut self,
        md: &MarketData,
        stablebond_mint: &Pubkey,
    ) -> Result<StrategyResult> {
        let mut sell_liquidity_usdc_amount = md
            .sell_liquidity_usdc_amount
            .ok_or_else(|| anyhow::anyhow!("Missing sell_liquidity_usdc_amount"))?;
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
        if sell_liquidity_usdc_amount == 0 {
            return Err(anyhow::anyhow!(
                "Sell liquidity in USDC is required for this strategy"
            ));
        }

        sell_liquidity_usdc_amount =
            match adjust_amount_for_slippage(sell_liquidity_usdc_amount, SLIPPAGE_BIPS) {
                Ok(adjusted_amount) => adjusted_amount,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Error adjusting amount for slippage: {}",
                        e
                    ));
                }
            };

        let max_usdc_token_amount_to_redeem = (sell_liquidity_usdc_amount
            .min(usdc_holdings_token_amount))
        .min(MAX_USDC_AMOUNT_PER_TRADE.to_token_amount(USDC_DECIMALS));

        let mut best_profit: f64 = 0.0;
        let mut best_usdc_amount = 0;
        let mut best_stablebond_amount = 0;
        let mut best_quote: Option<Quote> = None;

        let max_amount = max_usdc_token_amount_to_redeem;

        // Generate initial test points with exponential distribution
        let points: Vec<f64> = (0..INITIAL_POINTS)
            .map(|i| {
                let t = i as f64 / (INITIAL_POINTS - 1) as f64;
                let exp_t = t.powf(1.5); // Exponential distribution
                MIN_TRADE_PERCENT + (MAX_TRADE_PERCENT - MIN_TRADE_PERCENT) * exp_t
            })
            .collect();
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
                etherfuse_price_per_token,
                price_when_buying,
                stablebond_amount.to_ui_amount(STABLEBOND_DECIMALS),
            ) {
                Ok(profit) => profit - md.jito_tip_usd_price.unwrap_or(0.10),
                Err(e) => {
                    println!("Error calculating profit: {}. Skipping.", e);
                    continue;
                }
            };

            println!("\nTrade Analysis for BuyOnJupiterSellOnEtherfuse:");
            println!("Trade Size: {}% of max", trade_percent * 100.0);
            println!("USDC Amount: {}", usdc_amount.to_ui_amount(USDC_DECIMALS));
            println!("Price Impact: {:.2}%", price_impact * 100.0);
            println!(
                "Jito tip usd price: {}",
                md.jito_tip_usd_price.unwrap_or(0.10)
            );
            println!("Potential Profit: {}", potential_profit);
            println!("Buy price on jupiter: {}", price_when_buying);
            println!("Sell price on etherfuse: {}", etherfuse_price_per_token);
            println!("Stablebond: {:?}", stablebond_mint);

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

        println!("\n🏁 Search Complete");
        println!("Final best profit: {}", best_profit);
        println!(
            "Final USDC amount: {}",
            best_usdc_amount.to_ui_amount(USDC_DECIMALS)
        );
        println!(
            "Final Stablebond amount: {}",
            best_stablebond_amount.to_ui_amount(STABLEBOND_DECIMALS)
        );

        if best_quote.is_none() {
            return Err(anyhow::anyhow!("No profitable trades found"));
        }
        if best_profit < 1.0 {
            return Err(anyhow::anyhow!(
                "All trades were less than $1.00 USD profit"
            ));
        }
        let mut txs: Vec<VersionedTransaction> = Vec::new();
        if let Ok(buy_on_jupiter_tx) = self
            .jupiter_client
            .jupiter_swap_tx(best_quote.unwrap())
            .await
        {
            if let Ok(redeem_on_etherfuse_tx) = self
                .etherfuse_client
                .instant_bond_redemption_tx(best_stablebond_amount, stablebond_mint.clone())
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

impl Strategy for BuyOnEtherfuseSellOnJupiter {
    async fn process_market_data(
        &mut self,
        md: &MarketData,
        stablebond_mint: &Pubkey,
    ) -> Result<StrategyResult> {
        let usdc_holdings_token_amount = md
            .usdc_holdings_token_amount
            .ok_or_else(|| anyhow::anyhow!("Missing usdc_holdings_token_amount"))?;
        let purchase_liquidity_stablebond_amount = md
            .purchase_liquidity_stablebond_amount
            .ok_or_else(|| anyhow::anyhow!("Missing purchase_liquidity_stablebond_amount"))?;
        let etherfuse_price_per_token = md
            .etherfuse_price_per_token
            .ok_or_else(|| anyhow::anyhow!("Missing etherfuse_price_per_token"))?;

        if usdc_holdings_token_amount == 0 {
            return Err(anyhow::anyhow!(
                "USDC holdings are required for this strategy"
            ));
        }
        if purchase_liquidity_stablebond_amount == 0 {
            return Err(anyhow::anyhow!(
                "Stablebond purchase liquidity is required for this strategy"
            ));
        }

        let purchase_liquidity_ui_amount_ =
            purchase_liquidity_stablebond_amount.to_ui_amount(STABLEBOND_DECIMALS);
        let max_usdc_to_purchase_ui_amount =
            math::checked_float_mul(purchase_liquidity_ui_amount_, etherfuse_price_per_token)?.min(
                usdc_holdings_token_amount
                    .to_ui_amount(USDC_DECIMALS)
                    .min(MAX_USDC_AMOUNT_PER_TRADE),
            );
        let max_usdc_to_purchase_token_amount =
            max_usdc_to_purchase_ui_amount.to_token_amount(STABLEBOND_DECIMALS);

        let mut best_profit: f64 = 0.0;
        let mut best_usdc_amount = 0;
        let mut best_stablebond_amount = 0;
        let mut best_quote: Option<Quote> = None;

        let max_amount = max_usdc_to_purchase_token_amount;

        // Generate initial test points with exponential distribution
        let points: Vec<f64> = (0..INITIAL_POINTS)
            .map(|i| {
                let t = i as f64 / (INITIAL_POINTS - 1) as f64;
                let exp_t = t.powf(1.5); // Exponential distribution
                MIN_TRADE_PERCENT + (MAX_TRADE_PERCENT - MIN_TRADE_PERCENT) * exp_t
            })
            .collect();

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
                Ok(profit) => profit - md.jito_tip_usd_price.unwrap_or(0.10),
                Err(e) => {
                    println!("Error calculating profit: {}. Skipping.", e);
                    continue;
                }
            };

            println!("\nTrade Analysis for BuyOnEtherfuseSellOnJupiter:");
            println!("Trade Size: {}% of max", trade_percent * 100.0);
            println!("USDC Amount: {}", usdc_amount.to_ui_amount(USDC_DECIMALS));
            println!("Price Impact: {:.2}%", price_impact * 100.0);
            println!(
                "Jito tip usd price: {}",
                md.jito_tip_usd_price.unwrap_or(0.10)
            );
            println!("Potential Profit: {}", potential_profit);
            println!("Buy price on etherfuse: {}", etherfuse_price_per_token);
            println!("Sell price on jupiter: {}", price_per_token_when_selling);
            println!("Stablebond: {:?}", stablebond_mint);

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

        println!("\n🏁 Search Complete");
        println!("Final best profit: {}", best_profit);
        println!("Final USDC amount: {}", best_usdc_amount);
        println!("Final Stablebond amount: {}", best_stablebond_amount);

        if best_quote.is_none() {
            return Err(anyhow::anyhow!("No profitable trades found"));
        }

        if best_profit < 1.0 {
            return Err(anyhow::anyhow!(
                "All trades were less than $1.00 USD profit"
            ));
        }
        let mut txs: Vec<VersionedTransaction> = Vec::new();
        if let Ok(buy_on_etherfuse_tx) = self
            .etherfuse_client
            .purchase_tx(best_usdc_amount, stablebond_mint.clone())
            .await
        {
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

fn adjust_amount_for_slippage(amount: u64, bips: u64) -> Result<u64> {
    let subtraction =
        math::checked_mul(amount, bips).and_then(|product| math::checked_div(product, 10000))?;
    math::checked_sub(amount, subtraction)
}

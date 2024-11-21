use crate::market_data::MarketData;
use crate::math;
use crate::rate_limiter::RateLimiter;
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
use solana_account_decoder::parse_token::token_amount_to_ui_amount;
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
    pub rate_limiter: RateLimiter,
    pub jupiter_client: JupiterClient,
    pub switchboard_client: SwitchboardClient,
    pub etherfuse_client: EtherfuseClient,
}

impl BuyEtherfuseSellJupiter {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        jupiter_client: JupiterClient,
        keypair_filepath: String,
        rate_limiter: RateLimiter,
        switchboard_client: SwitchboardClient,
        etherfuse_client: EtherfuseClient,
    ) -> Self {
        BuyEtherfuseSellJupiter {
            rpc_client,
            keypair_filepath,
            rate_limiter,
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
    pub rate_limiter: RateLimiter,
    pub switchboard_client: SwitchboardClient,
    pub etherfuse_client: EtherfuseClient,
}

impl JupiterSellBuyEtherfuse {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        jupiter_client: JupiterClient,
        keypair_filepath: String,
        rate_limiter: RateLimiter,
        switchboard_client: SwitchboardClient,
        etherfuse_client: EtherfuseClient,
    ) -> Self {
        JupiterSellBuyEtherfuse {
            rpc_client,
            jupiter_client,
            keypair_filepath,
            rate_limiter,
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

        let mut best_profit = 0.0;
        let mut best_usdc_amount = 0;
        let mut best_stablebond_amount = 0;
        let mut best_quote: Option<Quote> = None;

        let mut left = MIN_USDC_AMOUNT;
        let mut right = max_usdc_token_amount_to_redeem;

        while left <= right {
            let mid_usdc = left + (right - left) / 2;
            let mid_stablebond = (mid_usdc as f64 / etherfuse_price_per_token) as u64;

            self.rate_limiter.wait_if_needed().await;

            let (price_when_buying, buy_quote) = loop {
                match self
                    .jupiter_client
                    .buy_quote(stablebond_mint, mid_usdc)
                    .await
                {
                    Ok(quote) => break quote,
                    Err(e) => {
                        println!("Error getting buy quote, retrying: {}", e);
                        self.rate_limiter.wait_if_needed().await;
                    }
                }
            };

            let potential_profit = math::profit_from_arb(
                price_when_buying,
                etherfuse_price_per_token,
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
        if best_quote.is_none() {
            return Err(anyhow::anyhow!(
                "No quote found for the strategy JupiterSellBuyEtherfuse",
            ));
        }
        println!(
            "Jupiter Buy -> Etherfuse Sell\nUSDC Amount: {}\nStablebond Amount: {}\nProfit: {}",
            token_amount_to_ui_amount(best_usdc_amount, USDC_DECIMALS).ui_amount_string,
            token_amount_to_ui_amount(best_stablebond_amount, STABLEBOND_DECIMALS).ui_amount_string,
            best_profit
        );
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

        let mut best_profit = 0.0;
        let mut best_usdc_amount = 0;
        let mut best_stablebond_amount = 0;
        let mut best_quote: Option<Quote> = None;

        let mut left = MIN_USDC_AMOUNT;
        let mut right = max_usdc_token_amount_to_purchase;

        while left <= right {
            let mid_usdc = left + (right - left) / 2;
            let mid_stablebond = (mid_usdc as f64 / etherfuse_price_per_token) as u64;

            self.rate_limiter.wait_if_needed().await;

            let (price_per_token_when_selling, sell_quote) = loop {
                match self
                    .jupiter_client
                    .sell_quote(stablebond_mint, mid_stablebond)
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
                etherfuse_price_per_token,
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

        if best_quote.is_none() {
            return Err(anyhow::anyhow!(
                "No quote found for the strategy BuyEtherfuseSellJupiter",
            ));
        }

        println!(
            "\nJupiter Sell -> Etherfuse Buy\nUSDC Amount: {}\nStablebond Amount: {}\nProfit: {}",
            token_amount_to_ui_amount(best_usdc_amount, USDC_DECIMALS).ui_amount_string,
            token_amount_to_ui_amount(best_stablebond_amount, STABLEBOND_DECIMALS).ui_amount_string,
            best_profit
        );
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

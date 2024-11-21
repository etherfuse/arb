use crate::constants::{MAX_STABLEBOND_AMOUNT_PER_TRADE, MAX_USDC_AMOUNT_PER_TRADE, USDC_MINT};
use crate::etherfuse::EtherfuseClient;
use crate::{jito::JitoClient, math, switchboard::SwitchboardClient};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::VersionedTransaction;
use spl_associated_token_account::{
    get_associated_token_address, get_associated_token_address_with_program_id,
};
use spl_token_2022::ID as SPL_TOKEN_2022_PROGRAM_ID;
use std::cmp::min;
use std::{str::FromStr, sync::Arc};

pub struct MarketData {
    pub etherfuse_price_per_token: Option<f64>,
    pub sell_liquidity_usdc_amount: Option<u64>,
    pub stablebond_holdings_token_amount: Option<u64>,
    pub purchase_liquidity_stablebond_amount: Option<u64>,
    pub usdc_holdings_token_amount: Option<u64>,
    pub jito_tip: Option<u64>,
    pub switchboard_update_tx: Option<VersionedTransaction>,
}

pub struct MarketDataBuilder {
    pub rpc_client: Arc<RpcClient>,
    pub wallet: Pubkey,
    pub etherfuse_client: EtherfuseClient,
    pub jito_client: JitoClient,
    pub switchboard_client: SwitchboardClient,
    pub etherfuse_price_per_token: Option<f64>,
    pub sell_liquidity_usdc_amount: Option<u64>,
    pub stablebond_holdings_token_amount: Option<u64>,
    pub purchase_liquidity_stablebond_amount: Option<u64>,
    pub usdc_holdings_token_amount: Option<u64>,
    pub jito_tip: Option<u64>,
    pub switchboard_update_tx: Option<VersionedTransaction>,
}

impl MarketDataBuilder {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        wallet: Pubkey,
        etherfuse_client: EtherfuseClient,
        jito_client: JitoClient,
        switchboard_client: SwitchboardClient,
    ) -> Self {
        MarketDataBuilder {
            rpc_client,
            wallet,
            etherfuse_client,
            jito_client,
            switchboard_client,
            etherfuse_price_per_token: None,
            sell_liquidity_usdc_amount: None,
            stablebond_holdings_token_amount: None,
            purchase_liquidity_stablebond_amount: None,
            usdc_holdings_token_amount: None,
            jito_tip: None,
            switchboard_update_tx: None,
        }
    }

    pub fn build(self) -> MarketData {
        MarketData {
            etherfuse_price_per_token: self.etherfuse_price_per_token,
            sell_liquidity_usdc_amount: self.sell_liquidity_usdc_amount,
            stablebond_holdings_token_amount: self.stablebond_holdings_token_amount,
            purchase_liquidity_stablebond_amount: self.purchase_liquidity_stablebond_amount,
            usdc_holdings_token_amount: self.usdc_holdings_token_amount,
            jito_tip: self.jito_tip,
            switchboard_update_tx: self.switchboard_update_tx,
        }
    }

    pub async fn with_etherfuse_price_per_token(mut self, stablebond_mint: &Pubkey) -> Self {
        self.etherfuse_price_per_token = Some(
            self.etherfuse_client
                .get_etherfuse_price(stablebond_mint)
                .await
                .unwrap(),
        );
        self
    }

    pub async fn with_sell_liquidity_usdc_amount(mut self, stablebond_mint: &Pubkey) -> Self {
        self.sell_liquidity_usdc_amount = Some(
            self.etherfuse_client
                .fetch_sell_liquidity_usdc_amount(stablebond_mint)
                .await
                .unwrap_or(0),
        );
        self
    }

    pub async fn with_purchase_liquidity_stablebond_amount(
        mut self,
        stablebond_mint: &Pubkey,
    ) -> Self {
        self.purchase_liquidity_stablebond_amount = Some(
            self.etherfuse_client
                .fetch_purchase_liquidity_stablebond_amount(stablebond_mint)
                .await
                .unwrap_or(0),
        );
        self
    }

    pub async fn with_stablebond_holdings_token_amount(mut self, stablebond_mint: &Pubkey) -> Self {
        self.stablebond_holdings_token_amount = Some(min(
            self.get_spl_token_22_balance(stablebond_mint)
                .await
                .unwrap_or(0),
            MAX_STABLEBOND_AMOUNT_PER_TRADE,
        ));
        self
    }

    pub async fn with_usdc_holdings_token_amount(mut self) -> Self {
        let usdc_mint = Pubkey::from_str(&USDC_MINT).unwrap();
        self.usdc_holdings_token_amount = Some(min(
            self.get_spl_token_balance(&usdc_mint).await.unwrap_or(0),
            MAX_USDC_AMOUNT_PER_TRADE,
        ));
        self
    }

    pub async fn with_jito_tip(mut self) -> Self {
        self.jito_tip = Some(self.jito_client.get_jito_tip().await.unwrap());
        self
    }

    pub async fn with_update_switchboard_oracle_tx(mut self, stablebond_mint: &Pubkey) -> Self {
        let payment_feed = self
            .etherfuse_client
            .fetch_payment_feed(stablebond_mint)
            .await
            .unwrap();

        let switchboard_public_feed = if payment_feed.quote_price_feed == Pubkey::default() {
            payment_feed.base_price_feed
        } else {
            payment_feed.quote_price_feed
        };

        self.switchboard_update_tx = Some(
            self.switchboard_client
                .get_update_switchboard_oracle_tx(switchboard_public_feed)
                .await
                .unwrap(),
        );
        self
    }

    async fn get_spl_token_balance(&self, mint: &Pubkey) -> Result<u64> {
        let user_token_account = get_associated_token_address(&self.wallet, mint);
        let token_account = self
            .rpc_client
            .get_token_account(&user_token_account)
            .await?;

        if let Some(token_account) = token_account {
            return Ok(math::to_token_amount(
                token_account.token_amount.ui_amount.unwrap_or(0.0),
                token_account.token_amount.decimals,
            )
            .unwrap());
        }
        return Ok(0);
    }

    async fn get_spl_token_22_balance(&self, mint: &Pubkey) -> Result<u64> {
        let user_token_account = get_associated_token_address_with_program_id(
            &self.wallet,
            &mint,
            &SPL_TOKEN_2022_PROGRAM_ID,
        );
        let token_account = self
            .rpc_client
            .get_token_account(&user_token_account)
            .await?;

        if let Some(token_account) = token_account {
            return Ok(math::to_token_amount(
                token_account.token_amount.ui_amount.unwrap_or(0.0),
                token_account.token_amount.decimals,
            )
            .unwrap());
        }
        return Ok(0);
    }
}

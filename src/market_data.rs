use crate::constants::USDC_MINT;
use crate::etherfuse::EtherfuseClient;
use crate::{jito::JitoClient, math};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::{
    get_associated_token_address, get_associated_token_address_with_program_id,
};
use spl_token_2022::ID as SPL_TOKEN_2022_PROGRAM_ID;
use stablebond_sdk::find_bond_pda;
use std::{str::FromStr, sync::Arc};

pub struct MarketData {
    pub etherfuse_price_per_token: Option<f64>,
    pub sell_liquidity_usdc_amount: Option<u64>,
    pub stablebond_holdings_token_amount: Option<u64>,
    pub usdc_holdings_token_amount: Option<u64>,
    pub jito_tip: Option<u64>,
}

pub struct MarketDataBuilder {
    pub rpc_client: Arc<RpcClient>,
    pub wallet: Pubkey,
    pub etherfuse_client: EtherfuseClient,
    pub jito_client: JitoClient,
    pub etherfuse_price_per_token: Option<f64>,
    pub sell_liquidity_usdc_amount: Option<u64>,
    pub stablebond_holdings_token_amount: Option<u64>,
    pub usdc_holdings_token_amount: Option<u64>,
    pub jito_tip: Option<u64>,
}

impl MarketDataBuilder {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        wallet: Pubkey,
        etherfuse_client: EtherfuseClient,
        jito_client: JitoClient,
    ) -> Self {
        MarketDataBuilder {
            rpc_client,
            wallet,
            etherfuse_client,
            jito_client,
            etherfuse_price_per_token: None,
            sell_liquidity_usdc_amount: None,
            stablebond_holdings_token_amount: None,
            usdc_holdings_token_amount: None,
            jito_tip: None,
        }
    }

    pub fn build(self) -> MarketData {
        MarketData {
            etherfuse_price_per_token: self.etherfuse_price_per_token,
            sell_liquidity_usdc_amount: self.sell_liquidity_usdc_amount,
            stablebond_holdings_token_amount: self.stablebond_holdings_token_amount,
            usdc_holdings_token_amount: self.usdc_holdings_token_amount,
            jito_tip: self.jito_tip,
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
                .fetch_sell_liquidity_usdc_amount(&find_bond_pda(*stablebond_mint).0)
                .await
                .unwrap(),
        );
        self
    }

    pub async fn with_stablebond_holdings_token_amount(mut self, stablebond_mint: &Pubkey) -> Self {
        self.stablebond_holdings_token_amount = Some(
            self.get_spl_token_22_balance(stablebond_mint)
                .await
                .unwrap(),
        );
        self
    }

    pub async fn with_usdc_holdings_token_amount(mut self) -> Self {
        let usdc_mint = Pubkey::from_str(&USDC_MINT).unwrap();
        self.usdc_holdings_token_amount =
            Some(self.get_spl_token_balance(&usdc_mint).await.unwrap());
        self
    }

    pub async fn with_jito_tip(mut self) -> Self {
        self.jito_tip = Some(self.jito_client.get_jito_tip().await.unwrap());
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
                token_account.token_amount.ui_amount.unwrap(),
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
                token_account.token_amount.ui_amount.unwrap(),
                token_account.token_amount.decimals,
            )
            .unwrap());
        }
        return Ok(0);
    }
}

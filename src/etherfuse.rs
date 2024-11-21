use anyhow::Result;
use serde::{Deserialize, Serialize};
use solana_program::{program_pack::Pack, system_program};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
    signer::Signer,
    transaction::VersionedTransaction,
};
use stablebond_sdk::accounts::Issuance;
use stablebond_sdk::instructions::{InstantBondRedemption, InstantBondRedemptionInstructionArgs};
use std::str::FromStr;
use std::sync::Arc;

use lazy_static::lazy_static;
use spl_associated_token_account::{
    get_associated_token_address, get_associated_token_address_with_program_id,
};
use spl_token::state::Account as TokenAccount;
use std::collections::HashMap;

use stablebond_sdk::{
    accounts::{Bond, PaymentFeed, SellLiquidity},
    find_bond_pda, find_issuance_pda, find_payment_feed_pda, find_payment_pda,
    find_sell_liquidity_pda,
    instructions::{PurchaseBond, PurchaseBondInstructionArgs},
};

use crate::args::InstantBondRedemptionArgs;
use crate::{
    args::PurchaseArgs, constants::USDC_MINT, field_as_string, transaction::build_and_sign_tx,
};

lazy_static! {
    static ref EXCHANGE_RATE_CONFIGS: HashMap<Pubkey, &'static str> = {
        let mut m = HashMap::new();
        m.insert(
            Pubkey::from_str("CETES7CKqqKQizuSN6iWQwmTeFRjbJR6Vw2XRKfEDR8f").unwrap(),
            "https://api.etherfuse.com/lookup/exchange_rate/usd_to_mxn",
        );
        m.insert(
            Pubkey::from_str("USTRYnGgcHAhdWsanv8BG6vHGd4p7UGgoB9NRd8ei7j").unwrap(),
            "https://api.etherfuse.com/lookup/exchange_rate/usd_to_usd",
        );
        m.insert(
            Pubkey::from_str("GiLTSeSFnNse7xQVYeKdMyckGw66AoRmyggGg1NNd4yr").unwrap(),
            "https://api.etherfuse.com/lookup/exchange_rate/usd_to_gbp",
        );
        m.insert(
            Pubkey::from_str("EuroszHk1AL7fHBBsxgeGHsamUqwBpb26oEyt9BcfZ6G").unwrap(),
            "https://api.etherfuse.com/lookup/exchange_rate/usd_to_eur",
        );
        m
    };
}

#[derive(Clone)]
pub struct EtherfuseClient {
    pub rpc_client: Arc<RpcClient>,
    pub keypair_filepath: String,
    pub etherfuse_api_url: String,
}

impl EtherfuseClient {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        keypair_filepath: String,
        etherfuse_api_url: String,
    ) -> Self {
        Self {
            rpc_client,
            keypair_filepath,
            etherfuse_api_url,
        }
    }

    pub fn signer(&self) -> Keypair {
        read_keypair_file(&self.keypair_filepath).expect("Unable to read keypair filepath")
    }

    pub async fn purchase_ix(&self, args: PurchaseArgs) -> Result<Instruction> {
        let ix_args = PurchaseBondInstructionArgs {
            amount: args.amount,
        };

        let bond_account = find_bond_pda(args.mint).0;
        let data = self.rpc_client.get_account_data(&bond_account).await?;
        let bond = Bond::from_bytes(&data).unwrap();

        let payment_feed_account = find_payment_feed_pda(bond.payment_feed_type).0;
        let data = self
            .rpc_client
            .get_account_data(&payment_feed_account)
            .await?;
        let payment_feed = PaymentFeed::from_bytes(&data).unwrap();

        let user_wallet = self.signer();
        let issuance_account = find_issuance_pda(bond_account, bond.issuance_number).0;
        let payment_account = find_payment_pda(issuance_account).0;
        let payment_mint_account = payment_feed.payment_mint;
        let mut payment_quote_price_feed_account = None;
        if payment_feed.quote_price_feed != Pubkey::default() {
            payment_quote_price_feed_account = Some(payment_feed.quote_price_feed);
        }

        let ix = PurchaseBond {
            user_wallet: user_wallet.pubkey(),
            user_token_account: get_associated_token_address_with_program_id(
                &user_wallet.pubkey(),
                &bond.mint,
                &spl_token_2022::id(),
            ),
            user_payment_token_account: get_associated_token_address(
                &user_wallet.pubkey(),
                &payment_mint_account,
            ),
            bond_account,
            issuance_account,
            mint_account: bond.mint,
            payment_account,
            payment_token_account: get_associated_token_address(
                &payment_account,
                &payment_mint_account,
            ),
            payment_mint_account,
            payment_feed_account,
            payment_base_price_feed_account: payment_feed.base_price_feed,
            payment_quote_price_feed_account,
            token2022_program: spl_token_2022::id(),
            associated_token_program: spl_associated_token_account::id(),
            token_program: spl_token::id(),
            system_program: system_program::id(),
        }
        .instruction(ix_args);

        Ok(ix)
    }

    pub async fn purchase_tx(&self, args: PurchaseArgs) -> Result<VersionedTransaction> {
        let ix = self.purchase_ix(args).await?;
        build_and_sign_tx(&self.rpc_client, &self.signer(), &[ix]).await
    }

    pub async fn instant_bond_redemption_ix(
        &self,
        args: InstantBondRedemptionArgs,
    ) -> Result<Instruction> {
        let bond_account = find_bond_pda(args.mint).0;
        let data = self.rpc_client.get_account_data(&bond_account).await?;
        let bond = Bond::from_bytes(&data).unwrap();

        let payment_feed_account = find_payment_feed_pda(bond.payment_feed_type).0;
        let data = self
            .rpc_client
            .get_account_data(&payment_feed_account)
            .await?;
        let payment_feed = PaymentFeed::from_bytes(&data).unwrap();
        let user_wallet = self.signer();
        let issuance_account = find_issuance_pda(bond_account, bond.issuance_number).0;
        let payment_mint_account = payment_feed.payment_mint;
        let mut payment_quote_price_feed_account = None;
        if payment_feed.quote_price_feed != Pubkey::default() {
            payment_quote_price_feed_account = Some(payment_feed.quote_price_feed);
        }

        let sell_liquidity_account = find_sell_liquidity_pda(bond_account).0;
        let sell_liuqidity_data = self
            .rpc_client
            .get_account_data(&sell_liquidity_account)
            .await?;
        let sell_liquidity = SellLiquidity::from_bytes(&sell_liuqidity_data).unwrap();
        let sell_liquidity_token_account =
            get_associated_token_address(&sell_liquidity_account, &payment_feed.payment_mint);
        let ix_args = InstantBondRedemptionInstructionArgs {
            amount: args.amount,
        };

        let ix = InstantBondRedemption {
            user_wallet: user_wallet.pubkey(),
            bond_account,
            issuance_account,
            user_bond_token_account: get_associated_token_address_with_program_id(
                &user_wallet.pubkey(),
                &bond.mint,
                &spl_token_2022::id(),
            ),
            sell_liquidity_account,
            sell_liquidity_token_account,
            fee_collector_wallet_token_account: get_associated_token_address(
                &sell_liquidity.fee_collector,
                &payment_mint_account,
            ),
            mint_account: bond.mint,
            user_payment_token_account: get_associated_token_address(
                &user_wallet.pubkey(),
                &payment_mint_account,
            ),
            payment_base_price_feed_account: payment_feed.base_price_feed,
            payment_quote_price_feed_account,
            payment_mint_account,
            payment_feed_account,
            token_program: spl_token::id(),
            token2022_program: spl_token_2022::id(),
            associated_token_program: spl_associated_token_account::id(),
            system_program: system_program::id(),
        }
        .instruction(ix_args);
        Ok(ix)
    }

    pub async fn instant_bond_redemption_tx(
        &self,
        args: InstantBondRedemptionArgs,
    ) -> Result<VersionedTransaction> {
        let ix = self.instant_bond_redemption_ix(args).await?;
        build_and_sign_tx(&self.rpc_client, &self.signer(), &[ix]).await
    }

    pub async fn get_etherfuse_price(&self, stablebond_mint: &Pubkey) -> Result<f64> {
        let url = format!(
            "{}/lookup/bonds/cost/{:?}",
            self.etherfuse_api_url, stablebond_mint
        );
        let res: BondCostResponse = reqwest::get(url).await?.json().await?;
        let token_value = res.bond_cost_in_payment_token;

        match self.get_etherfuse_exchange_rate(*stablebond_mint).await {
            Ok(exchange_rate) => {
                let price_in_usd = token_value / exchange_rate;
                Ok(price_in_usd)
            }
            Err(e) => {
                println!("Error getting etherfuse exchange rate: {:?}", e);
                Err(e)
            }
        }
    }

    async fn get_etherfuse_exchange_rate(&self, stablebond_mint: Pubkey) -> Result<f64> {
        let url = EXCHANGE_RATE_CONFIGS
            .get(&stablebond_mint)
            .ok_or_else(|| anyhow::anyhow!("Unsupported stablebond mint"))?;

        let res: ExchangeRateResponse = reqwest::get(*url).await?.json().await?;
        res.get_rate()
            .ok_or_else(|| anyhow::anyhow!("No valid exchange rate found in response"))
    }

    pub async fn fetch_sell_liquidity_usdc_amount(&self, stablebond_mint: &Pubkey) -> Result<u64> {
        let bond = find_bond_pda(*stablebond_mint).0;
        let usdc_token_account = get_associated_token_address(
            &find_sell_liquidity_pda(bond).0,
            &Pubkey::from_str(&USDC_MINT).unwrap(),
        );
        let usdc_token_account_data = self
            .rpc_client
            .get_account_data(&usdc_token_account)
            .await?;
        let usdc_token_account_info = TokenAccount::unpack(&usdc_token_account_data)?;
        Ok(usdc_token_account_info.amount)
    }

    pub async fn fetch_purchase_liquidity_stablebond_amount(
        &self,
        stablebond_mint: &Pubkey,
    ) -> Result<u64> {
        let bond = find_bond_pda(*stablebond_mint).0;
        let bond_account = self.rpc_client.get_account_data(&bond).await?;
        let data = Bond::from_bytes(&bond_account)?;
        let issuance = find_issuance_pda(bond, data.issuance_number).0;
        let data = self.rpc_client.get_account_data(&issuance).await?;
        let issuance = Issuance::from_bytes(&data)?;
        Ok(issuance.liquidity)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BondCostResponse {
    #[serde(with = "field_as_string")]
    pub bond_cost_in_payment_token: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExchangeRateResponse {
    #[serde(with = "field_as_string", default)]
    pub usd_to_mxn: f64,
    #[serde(with = "field_as_string", default)]
    pub usd_to_gbp: f64,
    #[serde(with = "field_as_string", default)]
    pub usd_to_eur: f64,
    #[serde(with = "field_as_string", default)]
    pub usd_to_usd: f64,
}

impl ExchangeRateResponse {
    pub fn get_rate(&self) -> Option<f64> {
        [
            self.usd_to_mxn,
            self.usd_to_gbp,
            self.usd_to_eur,
            self.usd_to_usd,
        ]
        .into_iter()
        .find(|&rate| rate > 0.0) // Changed from != 0.0 to > 0.0 for safety
    }
}

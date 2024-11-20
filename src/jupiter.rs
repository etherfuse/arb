use crate::args::JupiterQuoteArgs;
use crate::constants::USDC_MINT;
use crate::field_as_string;
use solana_sdk::signature::{read_keypair_file, Keypair};
use solana_sdk::signer::Signer;
use std::str::FromStr;

use {
    anyhow::Result,
    serde::{Deserialize, Serialize},
    solana_sdk::{
        pubkey::{ParsePubkeyError, Pubkey},
        transaction::VersionedTransaction,
    },
    std::collections::HashMap,
};

#[derive(Clone)]
pub struct JupiterClient {
    pub jupiter_quote_url: String,
    pub keypair_filepath: String,
}

impl JupiterClient {
    pub fn new(jupiter_quote_url: String, keypair_filepath: String) -> Self {
        JupiterClient {
            jupiter_quote_url,
            keypair_filepath,
        }
    }

    pub fn signer(&self) -> Keypair {
        read_keypair_file(self.keypair_filepath.clone())
            .expect(format!("No keypair found at {}", self.keypair_filepath).as_str())
    }

    pub fn sign_tx(&self, tx: VersionedTransaction) -> Result<VersionedTransaction> {
        let signed_tx = VersionedTransaction::try_new(tx.message, &[&self.signer()])
            .map_err(|e| anyhow::anyhow!("Failed to create transaction: {}", e))?;
        Ok(signed_tx)
    }

    pub async fn get_jupiter_quote(&self, args: JupiterQuoteArgs) -> Result<Quote> {
        let url = format!(
            "{}/quote?inputMint={}&outputMint={}&amount={}&slippageBps={}",
            self.jupiter_quote_url,
            args.input_mint,
            args.output_mint,
            args.amount,
            args.slippage_bps.unwrap_or(300),
        );

        let quote = maybe_jupiter_api_error(reqwest::get(url).await?.json().await?)?;
        Ok(quote)
    }

    pub async fn jupiter_swap_tx(&self, quote: Quote) -> Result<VersionedTransaction> {
        let url = format!("{}/swap", self.jupiter_quote_url);

        let request = SwapRequest {
            user_public_key: self.signer().pubkey(),
            wrap_and_unwrap_SOL: Some(true),
            prioritization_fee_lamports: None,
            as_legacy_transaction: Some(false),
            dynamic_compute_unit_limit: Some(true),
            quote_response: quote.clone(),
            context_slot: quote.context_slot,
            time_taken: quote.time_taken,
        };

        let response = maybe_jupiter_api_error::<SwapResponse>(
            reqwest::Client::builder()
                .build()?
                .post(url)
                .json(&request)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?,
        )?;

        fn decode(base64_transaction: String) -> Result<VersionedTransaction> {
            bincode::deserialize(&base64::decode(base64_transaction)?).map_err(|err| err.into())
        }

        let swap_transaction: VersionedTransaction = decode(response.swap_transaction)?;
        self.sign_tx(swap_transaction)
    }

    pub async fn sell_quote(&self, stablebond_mint: &Pubkey, amount: u64) -> Result<(f64, Quote)> {
        let jupiter_quote_args = JupiterQuoteArgs {
            input_mint: stablebond_mint.clone(),
            output_mint: Pubkey::from_str(USDC_MINT).unwrap(),
            amount,
            slippage_bps: Some(300),
        };
        let quote = self.get_jupiter_quote(jupiter_quote_args).await?;
        let jup_price_usd_to_token: f64 = quote.in_amount as f64 / quote.out_amount as f64;
        let jup_price_token_to_usd: f64 = 1 as f64 / jup_price_usd_to_token;
        Ok((jup_price_token_to_usd, quote))
    }

    pub async fn buy_quote(&self, stablebond_mint: &Pubkey, amount: u64) -> Result<(f64, Quote)> {
        let jupiter_quote_args = JupiterQuoteArgs {
            input_mint: Pubkey::from_str(USDC_MINT).unwrap(),
            output_mint: stablebond_mint.clone(),
            amount,
            slippage_bps: Some(300),
        };
        let quote = self.get_jupiter_quote(jupiter_quote_args).await?;
        let jup_price_token_to_usd: f64 = quote.in_amount as f64 / quote.out_amount as f64;
        Ok((jup_price_token_to_usd, quote))
    }
}

/// The Errors that may occur while using this crate
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("invalid pubkey in response data: {0}")]
    ParsePubkey(#[from] ParsePubkeyError),

    #[error("base64: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    #[error("bincode: {0}")]
    Bincode(#[from] bincode::Error),

    #[error("Jupiter API: {0}")]
    JupiterApi(String),

    #[error("serde_json: {0}")]
    SerdeJson(#[from] serde_json::Error),
}

/// Generic response with timing information
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response<T> {
    pub data: HashMap<String, T>,
    pub time_taken: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    #[serde(with = "field_as_string", rename = "id")]
    pub input_mint: Pubkey,
    #[serde(rename = "mintSymbol")]
    pub input_symbol: String,
    #[serde(with = "field_as_string", rename = "vsToken")]
    pub output_mint: Pubkey,
    #[serde(rename = "vsTokenSymbol")]
    pub output_symbol: String,
    pub price: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub input_mint: String,
    #[serde(with = "field_as_string")]
    pub in_amount: u64,
    pub output_mint: String,
    #[serde(with = "field_as_string")]
    pub out_amount: u64,
    #[serde(with = "field_as_string")]
    pub other_amount_threshold: u64,
    pub swap_mode: String,
    pub slippage_bps: u64,
    #[serde(with = "field_as_string")]
    pub price_impact_pct: f64,
    pub route_plan: Vec<RoutePlan>,
    pub context_slot: u64,
    pub time_taken: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlan {
    pub percent: u32,
    pub swap_info: SwapInfo,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapInfo {
    #[serde(with = "field_as_string")]
    amm_key: String,
    label: String,
    input_mint: String,
    output_mint: String,
    #[serde(with = "field_as_string")]
    in_amount: u64,
    #[serde(with = "field_as_string")]
    out_amount: String,
    #[serde(with = "field_as_string")]
    fee_amount: u64,
    fee_mint: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeInfo {
    #[serde(with = "field_as_string")]
    pub amount: u64,
    #[serde(with = "field_as_string")]
    pub mint: Pubkey,
    pub pct: f64,
}

/// Partially signed transactions required to execute a swap
#[derive(Clone, Debug)]
pub struct Swap {
    pub swap_transaction: VersionedTransaction,
}

#[derive(Default)]
pub struct SwapConfig {
    pub wrap_unwrap_sol: Option<bool>,
    pub fee_account: Option<Pubkey>,
    pub compute_unit_price_micro_lamports: Option<usize>,
    pub as_legacy_transaction: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(non_snake_case)]
struct SwapRequest {
    #[serde(with = "field_as_string")]
    user_public_key: Pubkey,
    wrap_and_unwrap_SOL: Option<bool>,
    prioritization_fee_lamports: Option<u64>,
    as_legacy_transaction: Option<bool>,
    dynamic_compute_unit_limit: Option<bool>,
    quote_response: Quote,
    context_slot: u64,
    time_taken: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwapResponse {
    swap_transaction: String,
}

pub type JupiterResult<T> = std::result::Result<T, Error>;

fn maybe_jupiter_api_error<T>(value: serde_json::Value) -> JupiterResult<T>
where
    T: serde::de::DeserializeOwned,
{
    #[derive(Deserialize)]
    struct ErrorResponse {
        error: String,
    }
    if let Ok(ErrorResponse { error }) = serde_json::from_value::<ErrorResponse>(value.clone()) {
        Err(Error::JupiterApi(error))
    } else {
        serde_json::from_value(value).map_err(|err| err.into())
    }
}

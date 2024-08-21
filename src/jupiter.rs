use crate::args::{JupiterPriceArgs, JupiterQuoteArgs};
use crate::field_as_string;
use crate::Arber;

use {
    anyhow::Result,
    serde::{Deserialize, Serialize},
    solana_sdk::{
        pubkey::{ParsePubkeyError, Pubkey},
        transaction::VersionedTransaction,
    },
    std::collections::HashMap,
};

impl Arber {
    pub async fn get_jupiter_price(&self, args: JupiterPriceArgs) -> Result<()> {
        let url = format!(
            "{}/price?ids={}&vsToken={}",
            self.jupiter_price_url.as_ref().unwrap(),
            args.input_mint,
            args.output_mint,
        );
        let res: Response<Price> = maybe_jupiter_api_error(reqwest::get(url).await?.json().await?)?;
        println!("Price: {:?}", res);
        Ok(())
    }

    pub async fn get_jupiter_quote(&self, args: JupiterQuoteArgs) -> Result<()> {
        let url = format!(
            "{}/quote?inputMint={}&outputMint={}&amount={}&onlyDirectRoutes={}&{}&slippageBps={}{}",
            self.jupiter_quote_url.as_ref().unwrap(),
            args.input_mint,
            args.output_mint,
            args.amount,
            false,
            false,
            args.slippage_bps,
            0,
        );

        let res: Response<Quote> = maybe_jupiter_api_error(reqwest::get(url).await?.json().await?)?;
        println!("Quote: {:?}", res);
        Ok(())
    }

    pub async fn jupiter_swap(&self, route: Quote, user_public_key: Pubkey) -> Result<Swap> {
        self.swap_with_config(route, user_public_key, SwapConfig::default())
            .await
    }

    async fn swap_with_config(
        &self,
        quote_response: Quote,
        user_public_key: Pubkey,
        _swap_config: SwapConfig,
    ) -> Result<Swap> {
        let url = format!("{}/swap", self.jupiter_quote_url.as_ref().unwrap());

        let request = SwapRequest {
            user_public_key,
            wrap_and_unwrap_SOL: Some(true),
            prioritization_fee_lamports: Some(self.priority_fee.unwrap_or(0)),
            as_legacy_transaction: Some(false),
            dynamic_compute_unit_limit: Some(true),
            quote_response: quote_response.clone(),
            context_slot: quote_response.context_slot,
            time_taken: quote_response.time_taken,
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

        Ok(Swap {
            swap_transaction: decode(response.swap_transaction)?,
        })
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

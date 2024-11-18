use crate::field_as_string;
use crate::Arber;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BondCostResponse {
    #[serde(with = "field_as_string")]
    pub bond_cost_in_payment_token: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExchangeRateResponse {
    #[serde(with = "field_as_string")]
    pub usd_to_mxn: f64,
}
impl Arber {
    pub async fn get_etherfuse_price(&self, mint: Pubkey) -> Result<f64> {
        let url = format!(
            "{}/lookup/bonds/cost/{:?}",
            self.etherfuse_url.as_ref().unwrap(),
            mint
        );
        let res: BondCostResponse = reqwest::get(url).await?.json().await?;
        let token_value = res.bond_cost_in_payment_token;

        let exchange_rate = self.get_etherfuse_exchange_rate().await?;
        // Convert MXN price to USD by dividing by the exchange rate
        let price_in_usd = token_value / exchange_rate;
        Ok(price_in_usd)
    }

    async fn get_etherfuse_exchange_rate(&self) -> Result<f64> {
        let url = "https://api.etherfuse.com/lookup/exchange_rate/usd_to_mxn";
        let res: ExchangeRateResponse = reqwest::get(url).await?.json().await?;
        let price = res.usd_to_mxn;
        Ok(price)
    }
}

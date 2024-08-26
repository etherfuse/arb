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
impl Arber {
    pub async fn get_etherfuse_price(&self, mint: Pubkey) -> Result<f64> {
        let url = format!(
            "{}/lookup/bonds/cost/{:?}",
            self.etherfuse_url.as_ref().unwrap(),
            mint
        );
        let res: BondCostResponse = reqwest::get(url).await?.json().await?;
        let price = res.bond_cost_in_payment_token;
        Ok(price)
    }
}

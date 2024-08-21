use crate::args::EtherfusePriceArgs;
use crate::field_as_string;
use crate::Arber;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BondCostResponse {
    #[serde(with = "field_as_string")]
    pub bond_cost_in_payment_token: f64,
}
impl Arber {
    pub async fn get_etherfuse_price(&self, args: EtherfusePriceArgs) -> Result<()> {
        let url = format!(
            "{}/lookup/bonds/cost/{:?}",
            self.etherfuse_url.as_ref().unwrap(),
            args.mint
        );
        let res: BondCostResponse = reqwest::get(url).await?.json().await?;
        let price = res.bond_cost_in_payment_token;
        let reciprocal = 1.0 / price;
        println!("Price: {}", reciprocal);
        Ok(())
    }
}

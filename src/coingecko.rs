use anyhow::Result;
use serde_json::Value as JsonValue;

pub async fn get_sol_price() -> Result<f64> {
    let resp = reqwest::get("https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd")
        .await?
        .text()
        .await?;
    let v: JsonValue = serde_json::from_str(&resp)?;
    Ok(v["solana"]["usd"].as_f64().unwrap_or(0.0))
}
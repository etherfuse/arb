use crate::Arber;

use anyhow::Result;
use base58::ToBase58;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::rpc_params;
use serde::Deserialize;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    signature::{Signature, Signer},
    transaction::Transaction,
};

impl Arber {
    pub async fn send_bundle(&self, txs: &[Transaction]) -> Result<()> {
        //bincode::serialize every transaction and convert it to Vec<String>
        let txs: Vec<String> = txs
            .iter()
            .map(|tx| bincode::serialize(tx).unwrap().to_base58())
            .collect::<Vec<String>>();

        let params = rpc_params![txs];
        let resp: Result<String, _> = self.jito_client.request("sendBundle", params).await;
        match resp {
            Ok(signature) => {
                println!("Signature: {:?}", signature);
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct Tip {
    pub time: String,
    pub landed_tips_25th_percentile: f64,
    pub landed_tips_50th_percentile: f64,
    pub landed_tips_75th_percentile: f64,
    pub landed_tips_95th_percentile: f64,
    pub landed_tips_99th_percentile: f64,
    pub ema_landed_tips_50th_percentile: f64,
}

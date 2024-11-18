use crate::Arber;

use anyhow::Result;
use base58::ToBase58;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::rpc_params;
use serde::Deserialize;
use solana_program::native_token::LAMPORTS_PER_SOL;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;
use solana_sdk::system_instruction;
use solana_sdk::transaction::VersionedTransaction;

impl Arber {
    pub async fn send_bundle(&self, txs: &[VersionedTransaction]) -> Result<()> {
        let tippers: Vec<String> = self
            .jito_client
            .request("getTipAccounts", rpc_params![""])
            .await?;

        let tip_ix = system_instruction::transfer(
            &self.signer().pubkey(),
            &Pubkey::try_from(tippers[0].to_string().as_str()).unwrap(),
            *self.jito_tip.read().unwrap(),
        );
        // print amount in sol not lamports
        println!(
            "SOL tip: {:?}",
            *self.jito_tip.read().unwrap() as f64 / LAMPORTS_PER_SOL as f64
        );
        let tip_tx = self.build_and_sign_tx(&[tip_ix]).await?;

        let txs: Vec<String> = [txs, &[tip_tx]]
            .concat()
            .iter()
            .map(|tx| bincode::serialize(tx).unwrap().to_base58())
            .collect::<Vec<String>>();

        let params = rpc_params![txs];
        let resp: Result<String, _> = self.jito_client.request("sendBundle", params).await;
        match resp {
            Ok(bundle) => {
                println!("https://explorer.jito.wtf/bundle/{bundle}");
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

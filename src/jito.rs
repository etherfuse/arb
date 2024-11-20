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

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct BundleStatus {
    bundle_id: String,
    status: String,
    landed_slot: Option<u64>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct BundleStatusResponse {
    context: Context,
    value: Vec<BundleStatus>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Context {
    slot: u64,
}

#[allow(dead_code)]
enum BundleStatusEnum {
    Landed,
    Failed,
    Pending,
    Invalid,
    Unknown,
    Timeout,
}

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
                match self.check_bundle_status(&bundle).await {
                    Ok(BundleStatusEnum::Landed) => println!("Bundle landed successfully"),
                    Ok(BundleStatusEnum::Failed) => println!("Bundle failed to land"),
                    Ok(BundleStatusEnum::Invalid) => println!("Bundle invalid"),
                    Ok(BundleStatusEnum::Pending) => println!("Bundle pending"),
                    Ok(BundleStatusEnum::Unknown) => println!("Bundle unknown"),
                    Ok(BundleStatusEnum::Timeout) => println!("Bundle timeout"),
                    Err(e) => eprintln!("Error checking bundle status: {:?}", e),
                }
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
            }
        }
        Ok(())
    }

    async fn check_bundle_status(&self, bundle_id: &str) -> Result<BundleStatusEnum> {
        let start_time = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(30);

        while start_time.elapsed() < timeout {
            let params = rpc_params![[bundle_id]];
            let response: Option<BundleStatusResponse> = self
                .jito_client
                .request("getInflightBundleStatuses", params)
                .await?;

            if let Some(resp) = response {
                if let Some(status) = resp.value.first() {
                    match status.status.as_str() {
                        "Landed" => return Ok(BundleStatusEnum::Landed),
                        "Failed" => return Ok(BundleStatusEnum::Failed),
                        "Pending" | "Invalid" => {
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                            if start_time.elapsed() >= timeout {
                                return Ok(BundleStatusEnum::Timeout);
                            }
                            continue;
                        }
                        _ => {
                            eprintln!("Unknown status: {}", status.status);
                            return Ok(BundleStatusEnum::Unknown);
                        }
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        Ok(BundleStatusEnum::Timeout)
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

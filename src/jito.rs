#![allow(dead_code)]
use crate::transaction::build_and_sign_tx;
use anyhow::Result;
use base58::ToBase58;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::rpc_params;
use serde::Deserialize;
use solana_program::native_token::LAMPORTS_PER_SOL;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair_file, Keypair};
use solana_sdk::signer::Signer;
use solana_sdk::system_instruction;
use solana_sdk::transaction::VersionedTransaction;
use std::sync::Arc;

#[derive(Clone)]
pub struct JitoClient {
    pub rpc_client: Arc<RpcClient>,
    pub keypair_filepath: String,
    pub jsonrpc_client: HttpClient,
}

impl JitoClient {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        jsonrpc_client: HttpClient,
        keypair_filepath: String,
    ) -> Self {
        Self {
            rpc_client,
            keypair_filepath,
            jsonrpc_client,
        }
    }

    pub fn signer(&self) -> Keypair {
        read_keypair_file(&self.keypair_filepath).expect("Failed to load keypair")
    }

    pub async fn send_bundle(&mut self, txs: &[VersionedTransaction]) -> Result<()> {
        let jito_tip = self.get_jito_tip().await?;

        let tippers: Vec<String> = self
            .jsonrpc_client
            .request("getTipAccounts", rpc_params![""])
            .await?;

        let tip_ix = system_instruction::transfer(
            &self.signer().pubkey(),
            &Pubkey::try_from(tippers[0].to_string().as_str()).unwrap(),
            jito_tip,
        );
        // print amount in sol not lamports
        println!("SOL tip: {:?}", jito_tip as f64 / LAMPORTS_PER_SOL as f64);
        let tip_tx = build_and_sign_tx(&self.rpc_client, &self.signer(), &[tip_ix]).await?;

        let txs: Vec<String> = [txs, &[tip_tx]]
            .concat()
            .iter()
            .map(|tx| bincode::serialize(tx).unwrap().to_base58())
            .collect::<Vec<String>>();

        let params = rpc_params![txs];
        let resp: Result<String, _> = self.jsonrpc_client.request("sendBundle", params).await;
        match resp {
            Ok(bundle) => {
                let now = chrono::Local::now();
                println!(
                    "[{}] https://explorer.jito.wtf/bundle/{bundle}",
                    now.format("%Y-%m-%d %H:%M:%S")
                );
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
                .jsonrpc_client
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

    pub async fn get_jito_tip(&self) -> Result<u64> {
        let client = reqwest::Client::new();
        if let Ok(response) = client
            .get("https://bundles.jito.wtf/api/v1/bundles/tip_floor")
            .send()
            .await
        {
            if let Ok(tips) = response.json::<Vec<Tip>>().await {
                for item in tips {
                    return Ok((item.ema_landed_tips_50th_percentile * (10_f64).powf(9.0)) as u64);
                }
            }
        }
        Err(anyhow::anyhow!("Failed to get jito tip"))
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

#[derive(Debug, Deserialize)]
struct BundleStatus {
    bundle_id: String,
    status: String,
    landed_slot: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct BundleStatusResponse {
    context: Context,
    value: Vec<BundleStatus>,
}

#[derive(Debug, Deserialize)]
struct Context {
    slot: u64,
}

enum BundleStatusEnum {
    Landed,
    Failed,
    Pending,
    Invalid,
    Unknown,
    Timeout,
}

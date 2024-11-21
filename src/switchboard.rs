use anyhow::{anyhow, Result};
use solana_program::{
    address_lookup_table::AddressLookupTableAccount, instruction::Instruction, pubkey::Pubkey,
};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    message::{v0::Message, VersionedMessage::V0},
    signature::{read_keypair_file, Keypair},
    signer::Signer,
    transaction::VersionedTransaction,
};
use std::sync::Arc;
use switchboard_on_demand_client;

#[derive(Clone)]
pub struct SwitchboardClient {
    pub rpc_client: Arc<RpcClient>,
    pub keypair_filepath: String,
}

impl SwitchboardClient {
    pub fn new(rpc_client: Arc<RpcClient>, keypair_filepath: String) -> Self {
        Self {
            rpc_client,
            keypair_filepath,
        }
    }

    fn signer(&self) -> Keypair {
        read_keypair_file(self.keypair_filepath.clone())
            .expect(format!("No keypair found at {}", self.keypair_filepath).as_str())
    }

    pub async fn get_update_switchboard_oracle_tx(
        &self,
        public_feed: Pubkey,
    ) -> Result<VersionedTransaction> {
        let (update_oracle_ix, lookup_tables) = self
            .fetch_oracle_feed(public_feed, self.signer().pubkey())
            .await?;

        let blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| anyhow!("Unable to get latest blockhash: {:?}", e))?;
        let msg = Message::try_compile(
            &self.signer().pubkey(),
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(250_000),
                ComputeBudgetInstruction::set_compute_unit_price(100000),
                update_oracle_ix.clone(),
            ],
            &lookup_tables,
            blockhash,
        )
        .map_err(|e| anyhow!("Unable to compile transaction message: {:?}", e))?;
        let tx = VersionedTransaction::try_new(V0(msg), &[&self.signer()])
            .map_err(|e| anyhow!("Unable to create versioned transaction: {:?}", e))?;
        Ok(tx)
    }

    async fn fetch_oracle_feed(
        &self,
        feed_public_key: Pubkey,
        payer: Pubkey,
    ) -> Result<(Instruction, Vec<AddressLookupTableAccount>)> {
        let feed_data =
            switchboard_on_demand_client::PullFeed::load_data(&self.rpc_client, &feed_public_key)
                .await?;
        let gws = switchboard_on_demand_client::QueueAccountData::load(
            &self.rpc_client,
            &feed_data.queue,
        )
        .await
        .map_err(|e| anyhow!("Unable to load queue account data: {:?}", e))?
        .fetch_gateways(&self.rpc_client)
        .await
        .map_err(|e| anyhow!("Unable to fetch gateways: {:?}", e))?;
        // test gateways and return first working one
        let mut gw = None;
        for trial_gw in gws {
            if trial_gw.test_gateway().await {
                gw = Some(trial_gw);
                break;
            }
        }
        let gateway = match gw {
            Some(gw) => gw,
            None => return Err(anyhow!("No gateways found")),
        };
        let ctx = switchboard_on_demand_client::SbContext::new();
        let (ix, _responses, _num_success, luts) =
            switchboard_on_demand_client::PullFeed::fetch_update_ix(
                ctx,
                &self.rpc_client,
                switchboard_on_demand_client::FetchUpdateParams {
                    feed: feed_public_key,
                    payer,
                    gateway,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| anyhow!("Unable to fetch update ix: {:?}", e))?;
        Ok((ix, luts))
    }
}

use anyhow::Result;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    signature::{Signature, Signer},
    transaction::Transaction,
};

use crate::Arber;

impl Arber {
    pub async fn send_and_confirm(&self, ixs: &[Instruction]) -> Result<Signature> {
        let priority_fee_ix = ComputeBudgetInstruction::set_compute_unit_price(100000);
        let mut ixs_with_priority = vec![priority_fee_ix];
        ixs_with_priority.extend_from_slice(ixs);
        let recent_blockhash = self.rpc_client.get_latest_blockhash().await?;
        let signing_keypair = self.signer();
        let mut tx: Transaction =
            Transaction::new_with_payer(&ixs_with_priority, Some(&self.signer().pubkey()));
        tx.sign(&[&signing_keypair], recent_blockhash);

        println!("Transaction signer pubkey: {:?}", self.signer().pubkey());

        match self.rpc_client.send_and_confirm_transaction(&tx).await {
            Ok(signature) => Ok(signature),
            Err(err) => {
                eprintln!("Error: {:?}", err);
                Err(err.into())
            }
        }
    }
}

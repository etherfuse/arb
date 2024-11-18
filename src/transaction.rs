use anyhow::Result;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    signature::{Signature, Signer},
    transaction::{Transaction, VersionedTransaction},
};

use crate::Arber;

impl Arber {
    pub fn sign_tx(&self, tx: VersionedTransaction) -> Result<VersionedTransaction> {
        let signed_tx = VersionedTransaction::try_new(tx.message, &[&self.signer()])
            .map_err(|e| anyhow::anyhow!("Failed to create transaction: {}", e))?;
        Ok(signed_tx)
    }

    pub async fn build_and_sign_tx(&self, ixs: &[Instruction]) -> Result<VersionedTransaction> {
        let priority_fee_ix = ComputeBudgetInstruction::set_compute_unit_price(200000);
        let mut ixs_with_priority = vec![priority_fee_ix];
        ixs_with_priority.extend_from_slice(ixs);
        let recent_blockhash = self.rpc_client.get_latest_blockhash().await?;
        let signing_keypair = self.signer();
        let tx: Transaction = Transaction::new_signed_with_payer(
            &ixs_with_priority,
            Some(&self.signer().pubkey()),
            &[&signing_keypair],
            recent_blockhash,
        );
        Ok(tx.into())
    }

    pub async fn sign_and_send_tx(&self, tx: VersionedTransaction) -> Result<Signature> {
        let signed_tx = self.sign_tx(tx)?;

        match self
            .rpc_client
            .send_and_confirm_transaction(&signed_tx)
            .await
        {
            Ok(signature) => {
                println!("Signature: {:?}", signature);
                Ok(signature)
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                Err(err.into())
            }
        }
    }

    pub async fn sign_and_send_ixs(&self, ixs: &[Instruction]) -> Result<Signature> {
        let tx = self.build_and_sign_tx(ixs).await?;
        match self.rpc_client.send_and_confirm_transaction(&tx).await {
            Ok(signature) => {
                println!("Signature: {:?}", signature);
                Ok(signature)
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                Err(err.into())
            }
        }
    }
}

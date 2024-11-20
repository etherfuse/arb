#![allow(dead_code)]
use anyhow::Result;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    signature::{Keypair, Signature, Signer},
    transaction::{Transaction, VersionedTransaction},
};

pub fn sign_tx(keypair: &Keypair, tx: VersionedTransaction) -> Result<VersionedTransaction> {
    let signed_tx = VersionedTransaction::try_new(tx.message, &[keypair])
        .map_err(|e| anyhow::anyhow!("Failed to create transaction: {}", e))?;
    Ok(signed_tx)
}

pub async fn build_and_sign_tx(
    rpc_client: &RpcClient,
    keypair: &Keypair,
    ixs: &[Instruction],
) -> Result<VersionedTransaction> {
    let priority_fee_ix = ComputeBudgetInstruction::set_compute_unit_price(100000);
    let mut ixs_with_priority = vec![priority_fee_ix];
    ixs_with_priority.extend_from_slice(ixs);
    let recent_blockhash = rpc_client.get_latest_blockhash().await?;
    let signing_keypair = keypair;
    let tx: Transaction = Transaction::new_signed_with_payer(
        &ixs_with_priority,
        Some(&keypair.pubkey()),
        &[&signing_keypair],
        recent_blockhash,
    );
    Ok(tx.into())
}

pub async fn sign_and_send_tx(
    rpc_client: &RpcClient,
    keypair: &Keypair,
    tx: VersionedTransaction,
) -> Result<Signature> {
    let signed_tx = sign_tx(keypair, tx)?;

    match rpc_client.send_and_confirm_transaction(&signed_tx).await {
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

pub async fn sign_and_send_ixs(
    rpc_client: &RpcClient,
    keypair: &Keypair,
    ixs: &[Instruction],
) -> Result<Signature> {
    let tx = build_and_sign_tx(rpc_client, keypair, ixs).await?;
    match rpc_client.send_and_confirm_transaction(&tx).await {
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

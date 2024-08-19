use clap::{arg, Parser};
use solana_program::pubkey::Pubkey;

#[derive(Parser, Debug)]
pub struct RunArgs {}

#[derive(Parser, Debug)]
pub struct PurchaseArgs {
    #[arg(
        value_name = "AMOUNT",
        help = "Amount of tokens to mint in token amount"
    )]
    pub amount: u64,

    #[arg(value_name = "MINT", help = "Public key of the stablebond mint")]
    pub mint: Pubkey,
}

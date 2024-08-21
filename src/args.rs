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

#[derive(Parser, Debug)]
pub struct EtherfusePriceArgs {
    #[arg(value_name = "MINT", help = "Public key of the stablebond mint")]
    pub mint: Pubkey,
}

#[derive(Parser, Debug)]
pub struct JupiterPriceArgs {
    #[arg(value_name = "INPUT_MINT", help = "Public key of the input mint")]
    pub input_mint: Pubkey,

    #[arg(value_name = "OUTPUT_MINT", help = "Public key of the output mint")]
    pub output_mint: Pubkey,
}

#[derive(Parser, Debug)]
pub struct JupiterQuoteArgs {
    #[arg(value_name = "INPUT_MINT", help = "Public key of the input mint")]
    pub input_mint: Pubkey,

    #[arg(value_name = "OUTPUT_MINT", help = "Public key of the output mint")]
    pub output_mint: Pubkey,

    #[arg(
        value_name = "AMOUNT",
        help = "Amount of tokens to swap in token amount"
    )]
    pub amount: u64,

    #[arg(
        value_name = "SLIPPAGE_BPS",
        help = "Slippage in basis points (10000 = 100%)"
    )]
    pub slippage_bps: u64,
}

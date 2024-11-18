use clap::{arg, Parser};
use solana_program::pubkey::Pubkey;

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
pub struct InstantBondRedemptionArgs {
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
    pub slippage_bps: Option<u64>,
}

#[derive(Parser, Debug)]
pub struct JupiterSwapArgs {
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
    pub slippage_bps: Option<u64>,
}

#[derive(Parser, Debug, Clone)]
pub struct RunArgs {
    #[arg(
        value_name = "ETHERFUSE_TOKEN",
        help = "Public key of the etherfuse token"
    )]
    pub etherfuse_token: Pubkey,

    #[arg(
        value_name = "SLIPPAGE_BPS",
        help = "Slippage in basis points (10000 = 100%)"
    )]
    pub slippage_bps: Option<u64>,
}

impl From<JupiterSwapArgs> for JupiterQuoteArgs {
    fn from(swap_args: JupiterSwapArgs) -> Self {
        Self {
            input_mint: swap_args.input_mint,
            output_mint: swap_args.output_mint,
            amount: swap_args.amount,
            slippage_bps: swap_args.slippage_bps,
        }
    }
}

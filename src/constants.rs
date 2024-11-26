pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const USDC_DECIMALS: u8 = 6;
pub const STABLEBOND_DECIMALS: u8 = 6;
pub const MIN_USDC_AMOUNT: u64 = 1000000;
pub const MAX_USDC_AMOUNT_PER_TRADE: f64 = 1000.0;
pub const MAX_STABLEBOND_AMOUNT_PER_TRADE: u64 = 20_000_000_000;

// Strategy constants
pub const MIN_TRADE_PERCENT: f64 = 0.01;
pub const MAX_TRADE_PERCENT: f64 = 1.0;
pub const INITIAL_POINTS: usize = 8;
pub const MAX_RETRIES: u32 = 3;
pub const RETRY_DELAY_MS: u64 = 60000;

pub const SLIPPAGE_BIPS: u64 = 20;

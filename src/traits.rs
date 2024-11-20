use crate::math;
use solana_account_decoder::parse_token::token_amount_to_ui_amount;

pub trait TokenAmountExt {
    fn to_ui_amount(&self, decimals: u8) -> f64;
}

impl TokenAmountExt for u64 {
    fn to_ui_amount(&self, decimals: u8) -> f64 {
        token_amount_to_ui_amount(*self, decimals)
            .ui_amount
            .unwrap()
    }
}

pub trait UiAmountExt {
    fn to_token_amount(&self, decimals: u8) -> u64;
}

impl UiAmountExt for f64 {
    fn to_token_amount(&self, decimals: u8) -> u64 {
        math::to_token_amount(*self, decimals).unwrap()
    }
}

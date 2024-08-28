#![allow(dead_code)]

use anyhow::{anyhow, Result};
use std::fmt::Display;

pub fn checked_as_f64<T>(arg: T) -> Result<f64>
where
    T: Display + num_traits::ToPrimitive + Clone,
{
    let option: Option<f64> = num_traits::NumCast::from(arg.clone());
    if let Some(res) = option {
        Ok(res)
    } else {
        Err(anyhow!("Math overflow"))
    }
}

pub fn checked_as_u64<T>(arg: T) -> Result<u64>
where
    T: Display + num_traits::ToPrimitive + Clone,
{
    let option: Option<u64> = num_traits::NumCast::from(arg.clone());
    if let Some(res) = option {
        Ok(res)
    } else {
        return Err(anyhow!("Math overflow"));
    }
}

pub fn checked_div<T>(arg1: T, arg2: T) -> Result<T>
where
    T: num_traits::PrimInt + Display,
{
    if let Some(res) = arg1.checked_div(&arg2) {
        Ok(res)
    } else {
        return Err(anyhow!("Math overflow"));
    }
}

pub fn checked_float_div<T>(arg1: T, arg2: T) -> Result<T>
where
    T: num_traits::Float + Display,
{
    if arg2 == T::zero() {
        return Err(anyhow!("Math overflow"));
    }
    let res = arg1 / arg2;
    if !res.is_finite() {
        return Err(anyhow!("Math overflow"));
    } else {
        Ok(res)
    }
}

pub fn checked_mul<T>(arg1: T, arg2: T) -> Result<T>
where
    T: num_traits::PrimInt + Display,
{
    if let Some(res) = arg1.checked_mul(&arg2) {
        Ok(res)
    } else {
        return Err(anyhow!("Math overflow"));
    }
}

pub fn checked_float_mul<T>(arg1: T, arg2: T) -> Result<T>
where
    T: num_traits::Float + Display,
{
    let res = arg1 * arg2;
    if !res.is_finite() {
        return Err(anyhow!("Math overflow"));
    } else {
        Ok(res)
    }
}

pub fn checked_powi(arg: f64, exp: i32) -> Result<f64> {
    let res = if exp > 0 {
        f64::powi(arg, exp)
    } else {
        // wrokaround due to f64::powi() not working properly on-chain with negative
        // exponent
        checked_float_div(1.0, f64::powi(arg, -exp))?
    };
    if res.is_finite() {
        Ok(res)
    } else {
        return Err(anyhow!("Math overflow"));
    }
}

pub fn to_ui_amount(amount: u64, decimals: u8) -> Result<f64> {
    checked_float_div(
        checked_as_f64(amount)?,
        checked_powi(10.0, decimals as i32)?,
    )
}

pub fn to_token_amount(ui_amount: f64, decimals: u8) -> Result<u64> {
    checked_as_u64(checked_float_mul(
        ui_amount,
        checked_powi(10.0, decimals as i32)?,
    )?)
}

use crate::constants::USDC_MINT;
use crate::field_as_string;
use crate::TradingEngine;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use solana_account_decoder::UiAccountEncoding;
use solana_client::rpc_config::RpcAccountInfoConfig;
use solana_client::rpc_config::RpcProgramAccountsConfig;
use solana_client::rpc_filter::Memcmp;
use solana_client::rpc_filter::RpcFilterType;
use solana_program::program_pack::Pack;
use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address;
use spl_token::state::Account as TokenAccount;
use stablebond_sdk::accounts::SellLiquidity;
use stablebond_sdk::find_sell_liquidity_pda;
use stablebond_sdk::{accounts::PaymentFeed, types::Discriminator};
use std::str::FromStr;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BondCostResponse {
    #[serde(with = "field_as_string")]
    pub bond_cost_in_payment_token: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExchangeRateResponse {
    #[serde(with = "field_as_string")]
    pub usd_to_mxn: f64,
}
impl TradingEngine {
    pub async fn get_etherfuse_price(&self, mint: Pubkey) -> Result<f64> {
        let url = format!(
            "{}/lookup/bonds/cost/{:?}",
            self.etherfuse_url.as_ref().unwrap(),
            mint
        );
        let res: BondCostResponse = reqwest::get(url).await?.json().await?;
        let token_value = res.bond_cost_in_payment_token;

        let exchange_rate = self.get_etherfuse_exchange_rate().await?;
        // Convert MXN price to USD by dividing by the exchange rate
        let price_in_usd = token_value / exchange_rate;
        Ok(price_in_usd)
    }

    async fn get_etherfuse_exchange_rate(&self) -> Result<f64> {
        let url = "https://api.etherfuse.com/lookup/exchange_rate/usd_to_mxn";
        let res: ExchangeRateResponse = reqwest::get(url).await?.json().await?;
        let price = res.usd_to_mxn;
        Ok(price)
    }

    pub async fn fetch_payment_feeds(&self) -> Result<Vec<PaymentFeed>> {
        let payment_feed_accounts = self
            .fetch_stablebond_accounts(Discriminator::PaymentFeed)
            .await?
            .into_iter()
            .map(|(_, account)| {
                PaymentFeed::from_bytes(&account.data)
                    .map(|payment_feed| payment_feed)
                    .map_err(|err| {
                        anyhow::anyhow!("Unable to parse payment feed account: {:?}", err)
                    })
            })
            .collect::<Result<Vec<PaymentFeed>>>()?;

        Ok(payment_feed_accounts)
    }

    pub async fn fetch_sell_liquidity_usdc_amount(&self, bond: Pubkey) -> Result<u64> {
        let sell_liquidity_account = self.fetch_sell_liquidity(bond).await?; //TODO: just grab the account directly and parse it.                                                                             // get token account in usdc
        let usdc_token_account = get_associated_token_address(
            &find_sell_liquidity_pda(bond).0,
            &Pubkey::from_str(&USDC_MINT).unwrap(),
        );
        let usdc_token_account_data = self
            .rpc_client
            .get_account_data(&usdc_token_account)
            .await?;
        let usdc_token_account_info = TokenAccount::unpack(&usdc_token_account_data)?;
        Ok(usdc_token_account_info.amount)
    }

    async fn fetch_sell_liquidity(&self, bond: Pubkey) -> Result<SellLiquidity> {
        let sell_liquidity_account = self
            .fetch_stablebond_accounts(Discriminator::SellLiquidity)
            .await?
            .into_iter()
            .filter(|(pubkey, _)| pubkey == &find_sell_liquidity_pda(bond).0)
            .map(|(_, account)| {
                SellLiquidity::from_bytes(&account.data)
                    .map(|sell_liquidity| sell_liquidity)
                    .map_err(|err| {
                        anyhow::anyhow!("Unable to parse sell liquidity account: {:?}", err)
                    })
            })
            .next() // Take only the first result
            .ok_or_else(|| anyhow::anyhow!("No sell liquidity account found"))??; // Handle None case and unwrap Result

        Ok(sell_liquidity_account)
    }

    async fn fetch_stablebond_accounts(
        &self,
        type_discriminator: Discriminator,
    ) -> Result<Vec<(Pubkey, solana_sdk::account::Account)>> {
        let accounts = self
            .rpc_client
            .get_program_accounts_with_config(
                &stablebond_sdk::ID,
                RpcProgramAccountsConfig {
                    with_context: None,
                    filters: Some(vec![RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                        0,
                        vec![type_discriminator as u8],
                    ))]),
                    account_config: RpcAccountInfoConfig {
                        encoding: Some(UiAccountEncoding::Binary),
                        commitment: None,
                        data_slice: None,
                        min_context_slot: None,
                    },
                },
            )
            .await?;
        Ok(accounts)
    }
}

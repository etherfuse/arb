use anyhow::Result;
use serde::{Deserialize, Serialize};
use solana_account_decoder::UiAccountEncoding;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_program::{program_pack::Pack, system_program};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
    signer::Signer,
    transaction::VersionedTransaction,
};
use stablebond_sdk::instructions::{InstantBondRedemption, InstantBondRedemptionInstructionArgs};
use std::cmp::min;
use std::str::FromStr;
use std::sync::Arc;

use spl_associated_token_account::{
    get_associated_token_address, get_associated_token_address_with_program_id,
};
use spl_token::state::Account as TokenAccount;

use stablebond_sdk::{
    accounts::{Bond, PaymentFeed, SellLiquidity},
    find_bond_pda, find_issuance_pda, find_payment_feed_pda, find_payment_pda,
    find_sell_liquidity_pda,
    instructions::{PurchaseBond, PurchaseBondInstructionArgs},
    types::Discriminator,
};

use crate::args::InstantBondRedemptionArgs;
use crate::{
    args::PurchaseArgs, constants::USDC_MINT, field_as_string, transaction::build_and_sign_tx,
};

#[derive(Clone)]
pub struct EtherfuseClient {
    pub rpc_client: Arc<RpcClient>,
    pub keypair_filepath: String,
    pub etherfuse_api_url: String,
}

impl EtherfuseClient {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        keypair_filepath: String,
        etherfuse_api_url: String,
    ) -> Self {
        Self {
            rpc_client,
            keypair_filepath,
            etherfuse_api_url,
        }
    }

    pub fn signer(&self) -> Keypair {
        read_keypair_file(&self.keypair_filepath).expect("Unable to read keypair filepath")
    }

    pub async fn purchase_ix(&self, args: PurchaseArgs) -> Result<Instruction> {
        let ix_args = PurchaseBondInstructionArgs {
            amount: args.amount,
        };

        let bond_account = find_bond_pda(args.mint).0;
        let data = self.rpc_client.get_account_data(&bond_account).await?;
        let bond = Bond::from_bytes(&data).unwrap();

        let payment_feed_account = find_payment_feed_pda(bond.payment_feed_type).0;
        let data = self
            .rpc_client
            .get_account_data(&payment_feed_account)
            .await?;
        let payment_feed = PaymentFeed::from_bytes(&data).unwrap();

        let user_wallet = self.signer();
        let issuance_account = find_issuance_pda(bond_account, bond.issuance_number).0;
        let payment_account = find_payment_pda(issuance_account).0;
        let payment_mint_account = payment_feed.payment_mint;
        let mut payment_quote_price_feed_account = None;
        if payment_feed.quote_price_feed != Pubkey::default() {
            payment_quote_price_feed_account = Some(payment_feed.quote_price_feed);
        }

        let ix = PurchaseBond {
            user_wallet: user_wallet.pubkey(),
            user_token_account: get_associated_token_address_with_program_id(
                &user_wallet.pubkey(),
                &bond.mint,
                &spl_token_2022::id(),
            ),
            user_payment_token_account: get_associated_token_address(
                &user_wallet.pubkey(),
                &payment_mint_account,
            ),
            bond_account,
            issuance_account,
            mint_account: bond.mint,
            payment_account,
            payment_token_account: get_associated_token_address(
                &payment_account,
                &payment_mint_account,
            ),
            payment_mint_account,
            payment_feed_account,
            payment_base_price_feed_account: payment_feed.base_price_feed,
            payment_quote_price_feed_account,
            token2022_program: spl_token_2022::id(),
            associated_token_program: spl_associated_token_account::id(),
            token_program: spl_token::id(),
            system_program: system_program::id(),
        }
        .instruction(ix_args);

        Ok(ix)
    }

    pub async fn purchase_tx(&self, args: PurchaseArgs) -> Result<VersionedTransaction> {
        let ix = self.purchase_ix(args).await?;
        build_and_sign_tx(&self.rpc_client, &self.signer(), &[ix]).await
    }

    pub async fn instant_bond_redemption_ix(
        &self,
        args: InstantBondRedemptionArgs,
    ) -> Result<Instruction> {
        let bond_account = find_bond_pda(args.mint).0;
        let data = self.rpc_client.get_account_data(&bond_account).await?;
        let bond = Bond::from_bytes(&data).unwrap();

        let payment_feed_account = find_payment_feed_pda(bond.payment_feed_type).0;
        let data = self
            .rpc_client
            .get_account_data(&payment_feed_account)
            .await?;
        let payment_feed = PaymentFeed::from_bytes(&data).unwrap();
        let user_wallet = self.signer();
        let issuance_account = find_issuance_pda(bond_account, bond.issuance_number).0;
        let payment_mint_account = payment_feed.payment_mint;
        let mut payment_quote_price_feed_account = None;
        if payment_feed.quote_price_feed != Pubkey::default() {
            payment_quote_price_feed_account = Some(payment_feed.quote_price_feed);
        }

        let sell_liquidity_account = find_sell_liquidity_pda(bond_account).0;
        let sell_liuqidity_data = self
            .rpc_client
            .get_account_data(&sell_liquidity_account)
            .await?;
        let sell_liquidity = SellLiquidity::from_bytes(&sell_liuqidity_data).unwrap();
        let sell_liquidity_token_account =
            get_associated_token_address(&sell_liquidity_account, &payment_feed.payment_mint);
        let data = self
            .rpc_client
            .get_account_data(&sell_liquidity_token_account)
            .await?;
        let sell_liquidity_token_account_account = TokenAccount::unpack(&data).unwrap();
        let token_amount = min(args.amount, sell_liquidity_token_account_account.amount);

        let ix_args = InstantBondRedemptionInstructionArgs {
            amount: token_amount,
        };

        let ix = InstantBondRedemption {
            user_wallet: user_wallet.pubkey(),
            bond_account,
            issuance_account,
            user_bond_token_account: get_associated_token_address_with_program_id(
                &user_wallet.pubkey(),
                &bond.mint,
                &spl_token_2022::id(),
            ),
            sell_liquidity_account,
            sell_liquidity_token_account,
            fee_collector_wallet_token_account: get_associated_token_address(
                &sell_liquidity.fee_collector,
                &payment_mint_account,
            ),
            mint_account: bond.mint,
            user_payment_token_account: get_associated_token_address(
                &user_wallet.pubkey(),
                &payment_mint_account,
            ),
            payment_base_price_feed_account: payment_feed.base_price_feed,
            payment_quote_price_feed_account,
            payment_mint_account,
            payment_feed_account,
            token_program: spl_token::id(),
            token2022_program: spl_token_2022::id(),
            associated_token_program: spl_associated_token_account::id(),
            system_program: system_program::id(),
        }
        .instruction(ix_args);
        Ok(ix)
    }

    pub async fn instant_bond_redemption_tx(
        &self,
        args: InstantBondRedemptionArgs,
    ) -> Result<VersionedTransaction> {
        let ix = self.instant_bond_redemption_ix(args).await?;
        build_and_sign_tx(&self.rpc_client, &self.signer(), &[ix]).await
    }

    pub async fn get_etherfuse_price(&self, mint: &Pubkey) -> Result<f64> {
        let url = format!("{}/lookup/bonds/cost/{:?}", self.etherfuse_api_url, mint);
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

    #[allow(dead_code)]
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

    pub async fn fetch_sell_liquidity_usdc_amount(&self, bond: &Pubkey) -> Result<u64> {
        let usdc_token_account = get_associated_token_address(
            &find_sell_liquidity_pda(*bond).0,
            &Pubkey::from_str(&USDC_MINT).unwrap(),
        );
        let usdc_token_account_data = self
            .rpc_client
            .get_account_data(&usdc_token_account)
            .await?;
        let usdc_token_account_info = TokenAccount::unpack(&usdc_token_account_data)?;
        Ok(usdc_token_account_info.amount)
    }

    #[allow(dead_code)]
    async fn fetch_sell_liquidity(&self, bond: &Pubkey) -> Result<SellLiquidity> {
        let sell_liquidity_account = self
            .fetch_stablebond_accounts(Discriminator::SellLiquidity)
            .await?
            .into_iter()
            .filter(|(pubkey, _)| pubkey == &find_sell_liquidity_pda(*bond).0)
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

    #[allow(dead_code)]
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

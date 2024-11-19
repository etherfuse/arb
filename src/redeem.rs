use std::cmp::min;

use anyhow::Result;

use solana_program::program_pack::Pack;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signer::Signer, system_program,
    transaction::VersionedTransaction,
};
use spl_associated_token_account::{
    get_associated_token_address, get_associated_token_address_with_program_id,
};
use spl_token::state::Account as TokenAccount;

use stablebond_sdk::{
    accounts::{Bond, PaymentFeed, SellLiquidity},
    find_bond_pda, find_issuance_pda, find_payment_feed_pda, find_sell_liquidity_pda,
    instructions::{InstantBondRedemption, InstantBondRedemptionInstructionArgs},
};

use crate::{args::InstantBondRedemptionArgs, Arber};

impl Arber {
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
        println!("Signer: {:?}", user_wallet.pubkey());
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
        println!(
            "Sell liquidity token account amount: {:?}",
            sell_liquidity_token_account_account.amount
        );
        let token_amount = min(args.amount, sell_liquidity_token_account_account.amount);
        println!("Token amount: {:?}", token_amount);

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

    pub async fn instant_bond_redemption(&self, args: InstantBondRedemptionArgs) -> Result<()> {
        let ix = self.instant_bond_redemption_ix(args).await?;
        self.sign_and_send_ixs(&[ix]).await?;
        Ok(())
    }

    pub async fn instant_bond_redemption_tx(
        &self,
        args: InstantBondRedemptionArgs,
    ) -> Result<VersionedTransaction> {
        let ix = self.instant_bond_redemption_ix(args).await?;
        self.build_and_sign_tx(&[ix]).await
    }
}

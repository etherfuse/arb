use anyhow::Result;
use solana_program::{pubkey::Pubkey, system_program};
use solana_sdk::signer::Signer;
use spl_associated_token_account::{
    get_associated_token_address, get_associated_token_address_with_program_id,
};
use stablebond_sdk::{
    accounts::{Bond, PaymentFeed},
    find_bond_pda, find_config_pda, find_issuance_pda, find_payment_feed_pda, find_payment_pda,
    instructions::{PurchaseBond, PurchaseBondInstructionArgs},
};

use crate::{args::PurchaseArgs, Arber};

impl Arber {
    pub async fn purchase(&self, args: PurchaseArgs) -> Result<()> {
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
            config_account: find_config_pda().0,
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

        match self.send_and_confirm(&[ix]).await {
            Ok(v) => {
                println!("Purchase successful: {:?}", v);
                Ok(())
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                return Err(err.into());
            }
        }
    }
}

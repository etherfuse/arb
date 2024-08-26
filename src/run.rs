use crate::args::RunArgs;
use crate::{Arber, PurchaseArgs};

use anyhow::Result;
use solana_sdk::transaction::VersionedTransaction;

impl Arber {
    pub async fn run(&self, args: RunArgs) -> Result<()> {
        // run a task that checks arb every 5 minutes
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            self.check_arb(args.clone()).await?;
        }
    }

    async fn check_arb(&self, args: RunArgs) -> Result<()> {
        let quote = self.get_jupiter_quote(args.clone().into()).await?;
        let jup_price_usd_to_token: f64 = quote.in_amount as f64 / quote.out_amount as f64;
        let jup_price_token_to_usd: f64 = 1 as f64 / jup_price_usd_to_token;
        let etherfuse_price_token_to_usd = self.get_etherfuse_price(args.input_mint).await?;
        println!(
            "jupiter price: {} \netherfuse price: {}",
            jup_price_token_to_usd, etherfuse_price_token_to_usd,
        );
        if jup_price_token_to_usd > etherfuse_price_token_to_usd {
            println!("Arb opportunity: jupiter token price > etherfuse price honored. Purchase tokens from etherfuse and sell on jupiter");
            let purchase_args = PurchaseArgs {
                amount: quote.out_amount,
                mint: args.input_mint,
            };
            let purchase_tx = self.purchase_tx(purchase_args).await?;
            let swap_tx = self.jupiter_swap_tx(args.clone().into()).await?;
            let txs: &[VersionedTransaction] = &[purchase_tx, swap_tx];
            self.send_bundle(txs).await?;
        } else {
            println!("Arb opportunity: etherfuse price honored < jupiter token price. Purchase tokens from jupiter and sell on etherfuse");
        }
        Ok(())
    }
}

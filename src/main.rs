mod args;
mod etherfuse;
mod field_as_string;
mod jito;
mod jupiter;
mod purchase;
mod transaction;

use anyhow::Result;
use args::*;
use clap::{arg, command, Parser, Subcommand};
use futures::StreamExt;
use jito::Tip;
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::native_token::lamports_to_sol;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    signature::{read_keypair_file, Keypair},
    transaction::VersionedTransaction,
};
use std::{sync::Arc, sync::RwLock};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

struct Arber {
    pub keypair_filepath: Option<String>,
    pub priority_fee: Option<u64>,
    pub rpc_client: Arc<RpcClient>,
    pub etherfuse_url: Option<String>,
    pub jupiter_quote_url: Option<String>,
    pub jupiter_price_url: Option<String>,
    pub jito_client: HttpClient,
    pub jito_tip: Arc<std::sync::RwLock<u64>>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Purchase a bond")]
    Purchase(PurchaseArgs),

    #[command(about = "Get etherfuse price of a bond")]
    GetEtherfusePrice(EtherfusePriceArgs),

    #[command(about = "Get jupiter price")]
    GetJupiterPrice(JupiterPriceArgs),

    #[command(about = "Get jupiter quote")]
    GetJupiterQuote(JupiterQuoteArgs),

    #[command(about = "Jupiter swap")]
    JupiterSwap(JupiterSwapArgs),

    #[command(about = "Test arb bot")]
    TestArb(TestArbArgs),
}

#[derive(Parser)]
#[command(about, version)]
struct Args {
    #[arg(
        long,
        value_name = "NETWORK_URL",
        help = "Network address of your RPC provider",
        global = true
    )]
    rpc: Option<String>,

    #[clap(
        global = true,
        short = 'C',
        long = "config",
        id = "PATH",
        help = "Filepath to config file."
    )]
    config_file: Option<String>,

    #[arg(
        long,
        value_name = "KEYPAIR_FILEPATH",
        help = "Filepath to signer keypair.",
        global = true
    )]
    keypair: Option<String>,

    #[arg(
        long,
        value_name = "MICROLAMPORTS",
        help = "Price to pay for compute units. If dynamic fees are enabled, this value will be used as the cap.",
        default_value = "100000",
        global = true
    )]
    priority_fee: Option<u64>,

    #[arg(
        long,
        value_name = "ETHERFUSE_API_URL",
        help = "URL to the Etherfuse API",
        default_value = "https://api.etherfuse.com",
        global = true
    )]
    etherfuse_url: Option<String>,

    #[arg(
        long,
        value_name = "JUPITER_QUOTE_API_URL",
        help = "URL to the Jupiter Quote API",
        default_value = "https://quote-api.jup.ag/v6",
        global = true
    )]
    jupiter_quote_url: Option<String>,

    #[arg(
        long,
        value_name = "JUPITER_PRICE_API_URL",
        help = "URL to the Jupiter Price API",
        default_value = "https://price.jup.ag/v4",
        global = true
    )]
    jupiter_price_url: Option<String>,

    #[arg(
        long,
        value_name = "JITO_BUNDLES_URL",
        help = "URL to the Jito Bundles API",
        default_value = "https://mainnet.block-engine.jito.wtf/api/v1/bundles",
        global = true
    )]
    jito_bundles_url: Option<String>,

    #[arg(
        long,
        value_name = "JITO_TIP",
        help = "Jito tip amount",
        default_value = "false",
        global = true
    )]
    use_jito: bool,

    #[command(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let cli_config = if let Some(config_file) = &args.config_file {
        solana_cli_config::Config::load(config_file).unwrap_or_else(|_| {
            eprintln!("error: Could not find config file `{}`", config_file);
            std::process::exit(1);
        })
    } else if let Some(config_file) = &*solana_cli_config::CONFIG_FILE {
        solana_cli_config::Config::load(config_file).unwrap_or_default()
    } else {
        solana_cli_config::Config::default()
    };

    let cluster = args.rpc.unwrap_or(cli_config.json_rpc_url);
    let default_keypair = args.keypair.unwrap_or(cli_config.keypair_path.clone());
    let rpc_client = RpcClient::new_with_commitment(cluster, CommitmentConfig::confirmed());
    let tip = Arc::new(RwLock::new(0_u64));
    let tip_clone = Arc::clone(&tip);

    if args.use_jito {
        let url = "ws://bundles-api-rest.jito.wtf/api/v1/bundles/tip_stream";
        let (ws_stream, _) = connect_async(url).await.unwrap();
        let (_, mut read) = ws_stream.split();

        tokio::spawn(async move {
            while let Some(message) = read.next().await {
                if let Ok(Message::Text(text)) = message {
                    if let Ok(tips) = serde_json::from_str::<Vec<Tip>>(&text) {
                        for item in tips {
                            let mut tip = tip_clone.write().unwrap();
                            *tip =
                                (item.ema_landed_tips_50th_percentile * (10_f64).powf(9.0)) as u64;
                            println!("Tip in SOL: {}", lamports_to_sol(*tip));
                        }
                    }
                }
            }
        });
    }

    let jito_client: HttpClient = HttpClientBuilder::default()
        .build(args.jito_bundles_url.clone().unwrap())
        .expect("Error");

    let arber = Arber::new(
        Arc::new(rpc_client),
        Some(default_keypair),
        args.priority_fee,
        args.etherfuse_url,
        args.jupiter_quote_url,
        args.jupiter_price_url,
        jito_client,
        tip,
    );

    //if the command is test arb and the tip is still 0, we wait until its not
    if let Commands::TestArb(_) = args.command {
        while *arber.jito_tip.read().unwrap() == 0 {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    match args.command {
        Commands::Purchase(purchase_args) => arber.purchase(purchase_args).await,
        Commands::GetEtherfusePrice(etherfuse_price_args) => {
            arber.get_etherfuse_price(etherfuse_price_args).await
        }
        Commands::GetJupiterPrice(jupiter_price_args) => {
            arber.get_jupiter_price(jupiter_price_args).await
        }
        Commands::GetJupiterQuote(jupiter_quote_args) => {
            let _ = arber.get_jupiter_quote(jupiter_quote_args).await;
            Ok(())
        }
        Commands::JupiterSwap(jupiter_swap_args) => arber.jupiter_swap(jupiter_swap_args).await,
        Commands::TestArb(test_arb_args) => {
            let test_arb_args_clone = test_arb_args.clone();
            let swap_tx = arber.jupiter_swap_tx(test_arb_args.into()).await?;
            let purchase_tx = arber.purchase_tx(test_arb_args_clone.into()).await?;
            let txs: &[VersionedTransaction] = &[swap_tx, purchase_tx];
            let res = arber.send_bundle(txs).await;
            println!("{:?}", res);
            Ok(())
        }
    }
}

impl Arber {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        keypair_filepath: Option<String>,
        priority_fee: Option<u64>,
        etherfuse_url: Option<String>,
        jupiter_quote_url: Option<String>,
        jupiter_price_url: Option<String>,
        jito_client: HttpClient,
        jito_tip: Arc<std::sync::RwLock<u64>>,
    ) -> Self {
        Self {
            rpc_client,
            keypair_filepath,
            priority_fee,
            etherfuse_url,
            jupiter_quote_url,
            jupiter_price_url,
            jito_client,
            jito_tip,
        }
    }

    pub fn signer(&self) -> Keypair {
        match self.keypair_filepath.clone() {
            Some(filepath) => read_keypair_file(filepath.clone())
                .expect(format!("No keypair found at {}", filepath).as_str()),
            None => panic!("No keypair provided"),
        }
    }
}

mod args;
mod etherfuse;
mod field_as_string;
mod jupiter;
mod purchase;
mod quote;
mod run;
mod send_and_confirm;

use anyhow::Result;
use args::*;
use clap::{arg, command, Parser, Subcommand};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    signature::{read_keypair_file, Keypair},
};
use std::sync::Arc;

struct Arber {
    pub keypair_filepath: Option<String>,
    pub priority_fee: Option<u64>,
    pub rpc_client: Arc<RpcClient>,
    pub etherfuse_url: Option<String>,
    pub jupiter_quote_url: Option<String>,
    pub jupiter_price_url: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Run arb bot")]
    Run(RunArgs),

    #[command(about = "Purchase a bond")]
    Purchase(PurchaseArgs),

    #[command(about = "Get etherfuse price of a bond")]
    GetEtherfusePrice(EtherfusePriceArgs),

    #[command(about = "Get jupiter price")]
    GetJupiterPrice(JupiterPriceArgs),

    #[command(about = "Get jupiter quote")]
    GetJupiterQuote(JupiterQuoteArgs),
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

    let arber = Arber::new(
        Arc::new(rpc_client),
        Some(default_keypair),
        args.priority_fee,
        args.etherfuse_url,
        args.jupiter_quote_url,
        args.jupiter_price_url,
    );

    match args.command {
        Commands::Run(run_args) => arber.run(run_args).await,
        Commands::Purchase(purchase_args) => arber.purchase(purchase_args).await,
        Commands::GetEtherfusePrice(etherfuse_price_args) => {
            arber.get_etherfuse_price(etherfuse_price_args).await
        }
        Commands::GetJupiterPrice(jupiter_price_args) => {
            arber.get_jupiter_price(jupiter_price_args).await
        }
        Commands::GetJupiterQuote(jupiter_quote_args) => {
            arber.get_jupiter_quote(jupiter_quote_args).await
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
    ) -> Self {
        Self {
            rpc_client,
            keypair_filepath,
            priority_fee,
            etherfuse_url,
            jupiter_quote_url,
            jupiter_price_url,
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

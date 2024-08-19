mod args;
mod purchase;
mod quote;
mod run;
mod send_and_confirm;
mod swap;

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
    pub dynamic_fee_url: Option<String>,
    pub dynamic_fee: bool,
    pub rpc_client: Arc<RpcClient>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Run arb bot")]
    Run(RunArgs),

    #[command(about = "Purchase a bond")]
    Purchase(PurchaseArgs),
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
        value_name = "DYNAMIC_FEE_URL",
        help = "RPC URL to use for dynamic fee estimation.",
        global = true
    )]
    dynamic_fee_url: Option<String>,

    #[arg(long, help = "Enable dynamic priority fees", global = true)]
    dynamic_fee: bool,

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
        args.dynamic_fee_url,
        args.dynamic_fee,
    );

    match args.command {
        Commands::Run(run_args) => arber.run(run_args).await,
        Commands::Purchase(purchase_args) => arber.purchase(purchase_args).await,
    }
}

impl Arber {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        keypair_filepath: Option<String>,
        priority_fee: Option<u64>,
        dynamic_fee_url: Option<String>,
        dynamic_fee: bool,
    ) -> Self {
        Self {
            rpc_client,
            keypair_filepath,
            priority_fee,
            dynamic_fee_url,
            dynamic_fee,
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

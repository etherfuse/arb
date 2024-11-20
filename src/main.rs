mod args;
mod constants;
mod etherfuse;
mod field_as_string;
mod jito;
mod jupiter;
mod market_data;
mod math;
mod rate_limiter;
mod strategy;
mod switchboard;
mod trading_engine;
mod traits;
mod transaction;

use crate::{
    etherfuse::EtherfuseClient, jito::JitoClient, jupiter::JupiterClient,
    switchboard::SwitchboardClient, trading_engine::TradingEngine,
};
use anyhow::Result;
use args::*;
use clap::{arg, command, Parser};
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use market_data::{MarketData, MarketDataBuilder};
use rate_limiter::RateLimiter;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::pubkey::Pubkey;
use solana_sdk::{
    commitment_config::CommitmentConfig, signature::read_keypair_file, signer::Signer,
};
use std::str::FromStr;
use std::sync::Arc;
use strategy::{BuyEtherfuseSellJupiter, JupiterSellBuyEtherfuse, StrategyEnum};

#[derive(Parser)]
#[command(about, version)]
struct Args {
    #[arg(
        long,
        value_name = "NETWORK_URL",
        help = "Network address of your RPC provider",
        default_value = "https://api.mainnet-beta.solana.com",
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
        value_name = "JITO_BUNDLES_URL",
        help = "URL to the Jito Bundles API",
        default_value = "https://slc.mainnet.block-engine.jito.wtf:443/api/v1/bundles",
        global = true
    )]
    jito_bundles_url: Option<String>,

    #[arg(
        long,
        value_name = "STABLEBOND_MINT",
        help = "The stablebond mint",
        global = true
    )]
    stablebond_mint: Option<String>,
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

    let keypair_filepath = args.keypair.unwrap_or(cli_config.keypair_path.clone());
    let wallet_keypair =
        read_keypair_file(keypair_filepath.clone()).expect("Error reading keypair file");
    let rpc_client = Arc::new(RpcClient::new_with_commitment(
        args.rpc.unwrap(),
        CommitmentConfig::confirmed(),
    ));

    let jito_jsonrpc_client: HttpClient = HttpClientBuilder::default()
        .build(args.jito_bundles_url.clone().unwrap())
        .expect("Error");
    let jito_client = JitoClient::new(
        rpc_client.clone(),
        jito_jsonrpc_client,
        keypair_filepath.clone(),
    );

    let etherfuse_client = EtherfuseClient::new(
        rpc_client.clone(),
        keypair_filepath.clone(),
        args.etherfuse_url.clone().unwrap(),
    );

    let jupiter_client = JupiterClient::new(
        args.jupiter_quote_url.clone().unwrap(),
        keypair_filepath.clone(),
    );

    let switchboard_client = SwitchboardClient::new(rpc_client.clone(), keypair_filepath.clone());

    let rate_limiter = RateLimiter::new(10, 10);

    let stablebond_mint = Pubkey::from_str(&args.stablebond_mint.unwrap()).unwrap();

    let market_data: MarketData = MarketDataBuilder::new(
        rpc_client.clone(),
        wallet_keypair.pubkey(),
        etherfuse_client.clone(),
        jito_client,
    )
    .with_etherfuse_price_per_token(&stablebond_mint)
    .await
    .with_sell_liquidity_usdc_amount(&stablebond_mint)
    .await
    .with_stablebond_holdings_token_amount(&stablebond_mint)
    .await
    .with_usdc_holdings_token_amount()
    .await
    .with_jito_tip()
    .await
    .build();

    let buy_etherfuse_sell_jupiter = BuyEtherfuseSellJupiter::new(
        rpc_client.clone(),
        jupiter_client.clone(),
        keypair_filepath.clone(),
        rate_limiter.clone(),
        switchboard_client.clone(),
        etherfuse_client.clone(),
    );

    let jupiter_sell_buy_etherfuse = JupiterSellBuyEtherfuse::new(
        rpc_client.clone(),
        jupiter_client.clone(),
        keypair_filepath.clone(),
        rate_limiter.clone(),
        switchboard_client.clone(),
        etherfuse_client.clone(),
    );

    TradingEngine::new()
        .add_strategy(StrategyEnum::BuyEtherfuseSellJupiter(
            buy_etherfuse_sell_jupiter,
        ))
        .add_strategy(StrategyEnum::JupiterSellBuyEtherfuse(
            jupiter_sell_buy_etherfuse,
        ))
        .run_strategies(&market_data, &stablebond_mint)
        .await;

    Ok(())
}

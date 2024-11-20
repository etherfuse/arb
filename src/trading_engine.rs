use crate::market_data::MarketData;
use crate::strategy::Strategy;
use crate::strategy::StrategyEnum;
use solana_sdk::pubkey::Pubkey;
pub struct TradingEngine {
    strategies: Vec<StrategyEnum>,
}

impl TradingEngine {
    pub fn new() -> Self {
        TradingEngine {
            strategies: Vec::new(),
        }
    }

    pub fn add_strategy(&mut self, strategy: StrategyEnum) -> &mut Self {
        self.strategies.push(strategy);
        self
    }

    pub async fn run_strategies(&mut self, md: &MarketData, stablebond_mint: &Pubkey) {
        loop {
            let mut results = Vec::new();
            for strategy in &mut self.strategies {
                if let Ok(result) = strategy.process_market_data(md, stablebond_mint).await {
                    results.push(result);
                }
            }
            println!("Results: {:?}", results);
            tokio::time::sleep(tokio::time::Duration::from_secs(60 * 2)).await;
        }
    }
}

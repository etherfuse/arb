use crate::market_data::MarketData;
use crate::strategy::{Strategy, StrategyEnum, StrategyResult};
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

    pub async fn run_strategies(
        &mut self,
        md: &MarketData,
        stablebond_mint: &Pubkey,
    ) -> Vec<StrategyResult> {
        let mut results: Vec<crate::strategy::StrategyResult> = Vec::new();
        for strategy in &mut self.strategies {
            match strategy.process_market_data(md, stablebond_mint).await {
                Ok(result) => results.push(result),
                Err(e) => println!("Error processing market data: {:?}", e),
            }
        }
        results
    }
}

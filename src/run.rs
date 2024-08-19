use anyhow::Result;
use colored::*;
use tokio::time::{sleep, Duration};

use crate::{args::RunArgs, Arber};

impl Arber {
    pub async fn run(&self, _args: RunArgs) -> Result<()> {
        loop {
            println!("{}", "Hello, world!".blue());
            sleep(Duration::from_secs(5)).await;
        }
    }
}

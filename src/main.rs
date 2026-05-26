mod backup;
mod cli;
mod config;
mod models;
mod reader;
mod scraper;
mod source;
mod storage;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    cli::run(cli).await
}

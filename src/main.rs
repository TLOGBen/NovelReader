mod backup;
mod catalog;
mod config;
mod library;
mod presentation;
mod utils;

use anyhow::Result;
use clap::Parser;

use presentation::cli::Cli;
use presentation::AppContext;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = crate::config::Config::load()?;
    let mut ctx = AppContext::bootstrap(config)?;
    presentation::cli::run(cli, &mut ctx).await
}

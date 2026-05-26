use anyhow::Result;

use crate::config::{self, Config};
use crate::presentation::cli::ConfigCmd;
use crate::presentation::AppContext;

pub async fn handle(action: ConfigCmd, ctx: &mut AppContext) -> Result<()> {
    match action {
        ConfigCmd::Show => {
            let text = toml::to_string_pretty(&ctx.config)?;
            println!("# {}", config::config_path()?.display());
            println!("{text}");
        }
        ConfigCmd::Set { key, value } => {
            // Reload from disk so concurrent edits aren't clobbered, mirror
            // pre-refactor behavior (cli.rs did its own Config::load here).
            let mut cfg = Config::load()?;
            let prev = cfg.set(&key, &value)?;
            cfg.save()?;
            ctx.config = cfg;
            match prev {
                Some(p) => println!("✓ {key} = {value} (was: {p})"),
                None => println!("✓ {key} = {value}"),
            }
        }
        ConfigCmd::Path => {
            println!("{}", config::config_path()?.display());
        }
    }
    Ok(())
}

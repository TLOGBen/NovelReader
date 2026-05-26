//! Presentation bounded context — CLI + TUI.
//!
//! 翻譯人類意圖 ↔ 其他 context 的 facade 編排。
//! 對 plugin layer 的 Published Language = stable CLI subcommand grammar
//! (`.claude/skills/` 三個 skill 都靠它運作)。

pub mod cli;
pub mod handlers;
pub mod reader;

use anyhow::Result;

use crate::catalog::service::scraper::Scraper;
use crate::config::Config;
use crate::library::dao::LibraryDb;

pub struct AppContext {
    pub db: LibraryDb,
    pub scraper: Scraper,
    pub config: Config,
}

impl AppContext {
    pub fn bootstrap(config: Config) -> Result<Self> {
        let db = LibraryDb::open()?;
        let scraper = Scraper::new()?;
        Ok(Self { db, scraper, config })
    }
}

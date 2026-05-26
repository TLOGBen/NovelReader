//! Presentation action layer — clap subcommand handlers.
//!
//! Each handler is `pub async fn handle(args, ctx: &mut AppContext) -> Result<()>`.
//! Handlers only call facade modules (catalog::facade / library::facade /
//! backup::facade / config); they do not directly import service or dao modules.

pub mod source;
pub mod search;
pub mod add;
pub mod shelf;
pub mod sync;
pub mod read;
pub mod tui;
pub mod config;
pub mod export;
pub mod import;
pub mod backup;
pub mod switch_source_core;

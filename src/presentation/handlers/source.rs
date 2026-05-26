use anyhow::{Context, Result};

use crate::catalog::facade;
use crate::catalog::BookSource;
use crate::presentation::cli::SourceCmd;
use crate::presentation::AppContext;

pub async fn handle(action: SourceCmd, ctx: &mut AppContext) -> Result<()> {
    match action {
        SourceCmd::Import { path } => {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            // 支援單一書源或書源陣列
            let count = if text.trim_start().starts_with('[') {
                let list: Vec<BookSource> = serde_json::from_str(&text)?;
                for s in &list {
                    facade::save_source(&mut ctx.db, s)?;
                }
                list.len()
            } else {
                let s: BookSource = serde_json::from_str(&text)?;
                facade::save_source(&mut ctx.db, &s)?;
                1
            };
            println!("已匯入 {count} 個書源");
        }
        SourceCmd::List => {
            let list = facade::list_sources(&ctx.db)?;
            if list.is_empty() {
                println!("（沒有書源，先用 `source import <file>` 匯入）");
            }
            for s in list {
                let group = s.book_source_group.as_deref().unwrap_or("-");
                let status = if s.enabled { "✓" } else { "✗" };
                println!("{status} [{group}] {} — {}", s.book_source_name, s.book_source_url);
            }
        }
    }
    Ok(())
}

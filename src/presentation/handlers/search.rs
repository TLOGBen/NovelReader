use anyhow::{anyhow, Result};

use crate::catalog::facade;
use crate::catalog::BookSource;
use crate::presentation::AppContext;

pub async fn handle(keyword: String, source: Option<String>, ctx: &mut AppContext) -> Result<()> {
    let sources: Vec<BookSource> = match source {
        Some(url) => vec![
            facade::get_source(&ctx.db, &url)?
                .ok_or_else(|| anyhow!("找不到書源: {url}"))?,
        ],
        None => facade::list_sources(&ctx.db)?
            .into_iter()
            .filter(|s| s.enabled)
            .collect(),
    };
    if sources.is_empty() {
        println!("沒有可用書源");
        return Ok(());
    }
    for src in &sources {
        println!("== {} ==", src.book_source_name);
        match ctx.scraper.search(src, &keyword).await {
            Ok(hits) => {
                if hits.is_empty() {
                    println!("  (no results)");
                }
                for h in hits {
                    println!(
                        "  {} / {} -> {}",
                        h.name,
                        h.author.as_deref().unwrap_or("-"),
                        h.book_url
                    );
                }
            }
            Err(e) => println!("  error: {e:#}"),
        }
    }
    Ok(())
}

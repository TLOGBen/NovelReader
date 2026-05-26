use anyhow::{anyhow, Result};

use crate::catalog::facade as catalog_facade;
use crate::library::facade as library_facade;
use crate::presentation::AppContext;

pub async fn handle(novel_id: i64, ctx: &mut AppContext) -> Result<()> {
    let novel = library_facade::get_novel(&ctx.db, novel_id)?
        .ok_or_else(|| anyhow!("找不到小說 #{novel_id}"))?;
    let src = catalog_facade::get_source(&ctx.db, &novel.source_url)?
        .ok_or_else(|| anyhow!("找不到書源 {}", novel.source_url))?;
    let toc_url = novel
        .toc_url
        .as_deref()
        .unwrap_or(&novel.book_url)
        .to_string();
    let n = catalog_facade::sync_toc(&mut ctx.db, &ctx.scraper, &src, novel_id, &toc_url).await?;
    println!("✓ 同步 {n} 章");
    Ok(())
}

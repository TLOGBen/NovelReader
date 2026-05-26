use anyhow::{anyhow, Result};

use crate::catalog::facade as catalog_facade;
use crate::library::facade as library_facade;
use crate::presentation::AppContext;

pub async fn handle(novel_id: i64, chapter_index: i64, ctx: &mut AppContext) -> Result<()> {
    let novel = library_facade::get_novel(&ctx.db, novel_id)?
        .ok_or_else(|| anyhow!("找不到小說 #{novel_id}"))?;
    let chapters = library_facade::list_chapters(&ctx.db, novel_id)?;
    let meta = chapters
        .iter()
        .find(|c| c.index == chapter_index)
        .ok_or_else(|| anyhow!("找不到第 {chapter_index} 章；先跑 sync"))?;
    // Try cache first.
    if let Some(ch) = library_facade::get_chapter(&ctx.db, novel_id, chapter_index)? {
        println!("# {}\n\n{}", ch.meta.name, ch.content);
        return Ok(());
    }
    let src = catalog_facade::get_source(&ctx.db, &novel.source_url)?
        .ok_or_else(|| anyhow!("找不到書源"))?;
    let content = catalog_facade::fetch_chapter_content(&ctx.scraper, &src, &meta.url).await?;
    library_facade::save_chapter_content(&mut ctx.db, novel_id, chapter_index, &content)?;
    println!("# {}\n\n{}", meta.name, content);
    Ok(())
}

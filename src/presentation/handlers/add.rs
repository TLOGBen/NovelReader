use anyhow::{anyhow, Result};

use crate::catalog::facade as catalog_facade;
use crate::library::facade as library_facade;
use crate::presentation::AppContext;

pub async fn handle(source: String, book_url: String, ctx: &mut AppContext) -> Result<()> {
    let src = catalog_facade::get_source(&ctx.db, &source)?
        .ok_or_else(|| anyhow!("找不到書源: {source}"))?;
    let novel = catalog_facade::fetch_novel_info(&ctx.scraper, &src, &book_url).await?;
    let id = library_facade::add_novel(&mut ctx.db, &novel)?;
    println!(
        "✓ 加入書架 (#{id}) {} / {}",
        novel.name,
        novel.author.as_deref().unwrap_or("-")
    );
    Ok(())
}

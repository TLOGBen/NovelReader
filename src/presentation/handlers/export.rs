use anyhow::Result;
use std::path::PathBuf;

use crate::backup;
use crate::presentation::AppContext;

pub async fn handle(path: PathBuf, ctx: &mut AppContext) -> Result<()> {
    let n = backup::export_to(&ctx.db, &path)?;
    println!("✓ 匯出 {n} 本書 → {}", path.display());
    Ok(())
}

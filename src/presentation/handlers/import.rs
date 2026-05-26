use anyhow::Result;
use std::path::PathBuf;

use crate::backup;
use crate::presentation::AppContext;

pub async fn handle(path: PathBuf, ctx: &mut AppContext) -> Result<()> {
    let s = backup::import_from(&mut ctx.db, &path)?;
    println!(
        "✓ 匯入 {} 本書（{} 含進度） ← {}",
        s.added,
        s.with_progress,
        path.display()
    );
    Ok(())
}

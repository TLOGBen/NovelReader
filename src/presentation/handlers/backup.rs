use anyhow::Result;

use crate::backup;
use crate::presentation::AppContext;

pub async fn handle(ctx: &mut AppContext) -> Result<()> {
    let receipt = backup::run_backup(&ctx.db, &ctx.config).await?;
    println!(
        "✓ 備份 {} 本書 [{}] → {}",
        receipt.novels, ctx.config.backup.backend, receipt.destination
    );
    Ok(())
}

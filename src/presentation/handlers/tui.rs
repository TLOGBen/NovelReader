use anyhow::Result;

use crate::presentation::AppContext;

pub async fn handle(novel_id: i64, ctx: &mut AppContext) -> Result<()> {
    crate::presentation::reader::run(&mut ctx.db, novel_id).await
}

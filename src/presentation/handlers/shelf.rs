use anyhow::Result;

use crate::library::facade;
use crate::presentation::AppContext;

pub async fn handle(ctx: &mut AppContext) -> Result<()> {
    let novels = facade::list_shelf(&ctx.db)?;
    if novels.is_empty() {
        println!("（書架空空，用 `add --source ... <book_url>` 加書）");
    }
    for n in novels {
        println!(
            "#{} {} / {} [{}]",
            n.id.unwrap_or(0),
            n.name,
            n.author.as_deref().unwrap_or("-"),
            n.source_url
        );
    }
    Ok(())
}

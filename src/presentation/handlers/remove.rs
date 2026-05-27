//! `remove` subcommand — hard-delete a novel from the shelf.
//!
//! Interactive mode (`--yes` absent) prints the book's name and asks `[y/N]`
//! (default N). `--yes` skips the prompt for scripted use.
//!
//! Cascade (chapters + progress) is handled atomically by
//! [`library::facade::delete_novel`] (single transaction).

use anyhow::{anyhow, Result};
use std::io::{self, BufRead, Write};

use crate::library::facade;
use crate::presentation::AppContext;

pub async fn handle(novel_id: i64, yes: bool, ctx: &mut AppContext) -> Result<()> {
    let novel = facade::get_novel(&ctx.db, novel_id)?
        .ok_or_else(|| anyhow!("找不到 novel_id={novel_id}"))?;

    if !yes {
        print!("確定要刪除《{}》嗎？[y/N] ", novel.name);
        io::stdout().flush().ok();
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        let answer = line.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            println!("已取消");
            return Ok(());
        }
    }

    facade::delete_novel(&mut ctx.db, novel_id)?;
    println!("已刪除《{}》", novel.name);
    Ok(())
}

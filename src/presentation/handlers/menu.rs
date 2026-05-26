//! TUI 主菜單入口 handler — `novel-looker`（無子命令）的 None 分流終點。
//!
//! REQ-001 Scenario 1：使用者執行 `novel-looker`（無參數）時必須進入 TUI 主
//! 菜單畫面，terminal 進入 alternate screen + raw mode。此 handler 構造
//! 一個以 `StubMenuScreen` 起手的 `App`、移交 owned `AppContext`，然後
//! 委派給 `tui::run_loop`。
//!
//! 真正的 `MenuScreen` 將由 TUI 群組（TASK-tui-01 / TASK-tui-06）實裝並
//! 取代此檔的 `StubMenuScreen`；此處只負責「進 run_loop」這一個接點，
//! handler 本身仍是薄的：一行構造 + 一行 await。
//!
//! Owned `AppContext`：menu handler 是唯一把 owned `AppContext` 移交給
//! `App` 的 entry；既有 CLI handler 仍使用 `&mut AppContext`，二者共存
//! 於 `cli::run` 的 dispatch。

use anyhow::Result;

use crate::presentation::handlers::tui::{run_loop, App, EntryMode, StubMenuScreen};
use crate::presentation::AppContext;

pub async fn handle(ctx: AppContext) -> Result<()> {
    let app = App::new(Box::new(StubMenuScreen), EntryMode::Menu, ctx);
    run_loop(app).await
}

//! CLI `switch-source` 子命令的薄 handler（REQ-005 Scenario 6）。
//!
//! 將 `switch_source_core::run` 的結果格式化為人類可讀訊息：
//! 成功印到 stdout、失敗印到 stderr 並以 exit 1 結束 process。
//! 失敗格式為 `換源失敗：<reason>` —— 這 zh-TW 前綴實際上是
//! `switch_source_core::run` 在 `with_context` 加上的；handler 只負責
//! 把 anyhow chain 印出來。
//!
//! 注意：handler 為「薄」—— 不重做任何業務邏輯，所有錯誤分類
//! (REQ-005 a/b/c/d/e) 都由 `switch_source_core` 決定。TUI shelf `s` 鍵
//! 也走同一個 `run()`；兩條路徑共享同一個 use case，atomicity 與
//! 成功/失敗判定一致。

use anyhow::Result;

use crate::presentation::handlers::switch_source_core;
use crate::presentation::AppContext;

pub async fn handle(
    novel_id: i64,
    new_book_url: String,
    source: String,
    ctx: &mut AppContext,
) -> Result<()> {
    match switch_source_core::run(ctx, novel_id, &source, &new_book_url).await {
        Ok(outcome) => {
            // `new_progress_idx` 是 0-based DB 內部欄位；訊息給人類看以 1-based。
            println!(
                "✓ 已換源 #{} 至 {}，進度重置到第 {} 章: {}",
                novel_id,
                new_book_url,
                outcome.new_progress_idx + 1,
                outcome.new_first_chapter_name,
            );
            Ok(())
        }
        Err(e) => {
            // 失敗訊息走 stderr；以 exit 1 結束。`{:#}` 印 anyhow chain
            // （含 with_context 的 zh-TW 前綴，例如「換源失敗：取得詳情頁失敗 (a)」）。
            eprintln!("{:#}", e);
            std::process::exit(1);
        }
    }
}

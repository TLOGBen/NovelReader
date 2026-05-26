# Tasks: presentation
**前置群組**：library, catalog, backup

## TASK-presentation-01: 建立 presentation/ 結構 + 搬 cli.rs 的 type 定義

**需求追溯**：REQ-001 (Scen 1.1, 1.2, 1.3), REQ-003 (Scen 3.1 — CLI grammar 不變), REQ-005
**目標**：`src/presentation/{mod.rs, cli.rs, handlers/mod.rs}` 建立；`Cli / Cmd / SourceCmd / ConfigCmd` clap struct + enum 從 `src/cli.rs` 搬到 `src/presentation/cli.rs`；舊 `src/cli.rs` 的 `run()` 函數**暫留**在原處，下個 task 拆。

**驗收標準**：
- [ ] `src/presentation/{mod.rs, cli.rs}` + `src/presentation/handlers/mod.rs` 建立
- [ ] `presentation/mod.rs` doc：`//! Presentation: CLI + TUI 翻譯人類意圖。對 plugin layer 的 PL = CLI subcommand grammar。`
- [ ] `presentation/cli.rs` 含 `Cli / Cmd / SourceCmd / ConfigCmd` 完整定義（含所有 clap derive、help text）
- [ ] `src/main.rs` 改為 `use crate::presentation::cli::Cli;`
- [ ] cargo build 通過、`cargo run -- --help` 輸出與重構前**逐字一致**

### 步驟

#### 建立 presentation 模組
- [ ] `mkdir -p src/presentation/handlers`
- [ ] `src/presentation/mod.rs`：doc + `pub mod cli; pub mod handlers; pub mod reader;`（reader 下 task 搬）
- [ ] `src/presentation/handlers/mod.rs`：佔位
- [ ] `src/main.rs`：移除 `mod cli;`，加 `mod presentation;`

#### 搬 type 定義
- [ ] 從 `src/cli.rs` 剪 `Cli / Cmd / SourceCmd / ConfigCmd` 所有 struct + enum + derive
- [ ] 貼到 `src/presentation/cli.rs`，含 `use clap::*` 等 imports
- [ ] **暫不搬** `pub async fn run(cli: Cli)`，下個 task 拆 handlers
- [ ] `src/cli.rs` 暫保留 `run()` 函數 + `use crate::presentation::cli::*;` 補充 type import

#### 驗證
- [ ] `cargo build`
- [ ] `cargo run -- --help`：subcommand 列表與「help text」與重構前一致（diff 比對）

---

## TASK-presentation-02: 拆 run() 為 handlers/*.rs（12 個 subcommand 各一檔）

**需求追溯**：REQ-001 (Scen 1.3), REQ-002 (Scen 2.4 — handler 是 application service / 跨 context facade 編排), REQ-003 (Scen 3.1), REQ-004 (all)
**目標**：把 `src/cli.rs` 的 `run()` match arms 拆成 12 個 handler 函數，分檔到 `src/presentation/handlers/{source, search, add, shelf, sync, read, tui, config, export, import, backup}.rs`。每個 handler 函數簽名 `pub async fn handle(args, ctx: &AppContext) -> Result<()>`。建立 `AppContext` struct 在 `presentation/mod.rs` 內，持有 `LibraryDb + Scraper + Config`（main.rs 一次性 wire up）。

**驗收標準**：
- [ ] `src/presentation/handlers/*.rs` 共 11 個檔案（source.rs 處理 SourceCmd::{Import, List}，config.rs 同理處理 ConfigCmd），加 `mod.rs` 共 12 檔
- [ ] 每個 handler 內部呼**對應 context 的 facade**（不直接呼 service 或 dao）
- [ ] `presentation/mod.rs` 定義 `pub struct AppContext { pub db: LibraryDb, pub scraper: Scraper, pub config: Config }`
- [ ] `src/cli.rs`（root）刪除
- [ ] `src/main.rs` 改為一次性 wire AppContext，呼 `presentation::cli::run(cli, &mut ctx).await`
- [ ] `presentation::cli::run` 內部變 match arm → `handlers::xxx::handle(args, ctx)` dispatch
- [ ] 所有 e2e（E1-E11）通過

### 步驟

#### 建立 AppContext
- [ ] `presentation/mod.rs` 加 struct AppContext + `impl AppContext { pub fn bootstrap(config) -> Result<Self> }`

#### 重寫 main.rs
- [ ] `src/main.rs`：
  ```rust
  mod backup;
  mod catalog;
  mod config;
  mod library;
  mod presentation;
  mod utils;

  use anyhow::Result;
  use clap::Parser;
  use presentation::cli::Cli;
  use presentation::AppContext;

  #[tokio::main]
  async fn main() -> Result<()> {
      let cli = Cli::parse();
      let config = crate::config::Config::load()?;
      let mut ctx = AppContext::bootstrap(config)?;
      presentation::cli::run(cli, &mut ctx).await
  }
  ```

#### 清理 storage.rs alias（TRANSITION marker 清零）
- [ ] grep `use crate::storage::Storage` 全 codebase，每處改為 `use crate::library::dao::LibraryDb`（含 reader.rs / handlers / 任何 still-pointing 處）
- [ ] `rm src/storage.rs`
- [ ] `src/main.rs` 移除 `mod storage;`
- [ ] 驗證：`grep -rnE "_legacy|legacy_|TRANSITION:|MOVED:" src/`：無輸出

#### 拆 handlers
依序為每個 subcommand 建立 `src/presentation/handlers/{name}.rs`，內含 `pub async fn handle(args, ctx: &mut AppContext) -> Result<()>`：
- [ ] `source.rs` — SourceCmd::{Import, List}
- [ ] `search.rs` — Cmd::Search
- [ ] `add.rs`
- [ ] `shelf.rs`
- [ ] `sync.rs`
- [ ] `read.rs`
- [ ] `tui.rs` — 呼 `presentation::reader::run`（下 task 搬）
- [ ] `config.rs` — ConfigCmd::{Show, Set, Path}
- [ ] `export.rs`
- [ ] `import.rs`
- [ ] `backup.rs` — 呼 `crate::backup::facade::run_backup(&ctx.db, &ctx.config)`

每個 handler 內**只呼 facade**（catalog::facade::* / library::facade::* / backup::facade::* / 與 config::*）；不直接 import service 或 dao 模組。

#### 改 presentation/cli.rs 的 run()
- [ ] `pub async fn run(cli: Cli, ctx: &mut AppContext) -> Result<()>`
- [ ] match cli.cmd → dispatch handlers::xxx::handle

#### 移除舊 cli.rs
- [ ] `rm src/cli.rs`

#### 驗證
- [ ] `cargo build`
- [ ] `cargo run -- --help` 與基線完全一致
- [ ] 全套 e2e 跑（E1, E3, E4, E5, E6, E7, E8 — TUI 留下個 task 後跑）
- [ ] `grep -rnE "use crate::(catalog|library|backup)::(service|dao)" src/presentation/handlers/`：無輸出（handler 只呼 facade）

---

## TASK-presentation-03: 搬 reader.rs → presentation/reader.rs

**需求追溯**：REQ-001 (Scen 1.3), REQ-004 (TUI 子集), test.md 邊界「Reading session state 暫居 Presentation 不洩」
**目標**：`src/reader.rs` 搬到 `src/presentation/reader.rs`；功能完全等價（不重構 ratatui 內部）；加 doc comment 註明 ReaderApp 含 Reading session state 待 OQ-2 觸發時拆出。

**驗收標準**：
- [ ] `src/presentation/reader.rs` 存在，內容等同舊 reader.rs（最多 import 路徑微調）
- [ ] `src/reader.rs` 已刪
- [ ] `src/main.rs` 移除 `mod reader;`
- [ ] `cargo run -- tui N`：開啟 TUI，j/k 切換章節正常，q 離開（不 crash、不 panic）
- [ ] 對 Library DAO 的呼叫改透過 facade（保持 handler/reader 都不直接呼 dao 的原則）

### 步驟

#### 搬檔
- [ ] `mv src/reader.rs src/presentation/reader.rs`
- [ ] 更新 import：`use crate::storage::Storage` → `use crate::library::dao::LibraryDb`；`use crate::scraper::Scraper` → `use crate::catalog::service::scraper::Scraper`
- [ ] 內部 `store.get_chapter / save_progress / list_chapters` 等改為 `library::facade::*` 呼叫
- [ ] 加 doc comment：
  ```rust
  //! ratatui TUI reader.
  //! ReaderApp 內含 reading session state（current_chapter, scroll, content cache）—
  //! 此屬 Reading session 概念，未來若拆出 Reading bounded context（OQ-2 觸發條件：
  //! annotation / highlight / 多 session）會搬到 reading/ 目錄。目前居此處。
  ```

#### 更新 main.rs
- [ ] 移除 `mod reader;`（已透過 presentation 路徑可達）

#### 驗證
- [ ] `cargo build`
- [ ] `cargo run -- tui $NID`：開啟 TUI，j 下章 / k 上章 / J 翻頁 / q 離開都正常
- [ ] 切到某中段章節（idx 500 或任意），文字顯示完整
- [ ] **Reading session state 不洩**：`grep -rn "scroll_offset\|chapter_index" src/library/ 2>/dev/null` 只應出現在 `ReadProgress` struct 定義與 DAO 序列化處（透過整包 ReadProgress 傳遞）
- [ ] **Cache hit 不重抓 (E7b)**：第二次跑 `RUST_LOG=debug cargo run -- read $NID 0 2>&1 | grep fetch_chapter_content`，應無輸出

---

## TASK-presentation-04: 清理 + 全 e2e + plugin 不變 + commit

**需求追溯**：REQ-001 (Scen 1.3), REQ-005, REQ-006 (Scen 6.1, 6.2, 6.3), test.md E1-E11
**目標**：移除所有過渡 `pub use` 別名與 legacy 殘留；跑完所有 e2e；確認 `.claude/skills/` 一字未動；commit + push。

**驗收標準**：
- [ ] `src/` 第一層 `.rs` 檔只剩 `main.rs` 與 `config.rs`
- [ ] 全 codebase 無 `_legacy` / `// TODO: remove` / 過渡 doc comment 殘留
- [ ] `cargo build --bin novel-looker` 與 `cargo build --release` 都通過，warning ≤ 基線（2 條 dead_code）
- [ ] E1-E11 全綠（見 test.md 表）
- [ ] `git diff --stat .claude/skills/` 無輸出
- [ ] `python3 .claude/skills/legado-converter/scripts/convert.py ...`：對 yckceo 7321 重跑成功
- [ ] commit message 清楚描述「DDD refactor: 拆 4 context × 5 layer」
- [ ] push 到 main

### 步驟

#### 清理
- [ ] `grep -rn "_legacy\|TODO: remove\|MOVED:" src/`：把過渡註解清掉
- [ ] 確認 `src/storage.rs`, `src/cli.rs`, `src/reader.rs`, `src/scraper.rs`, `src/backup.rs`（root）, `src/source/`, `src/models.rs` 都已刪除
- [ ] `ls src/`：只剩 `main.rs / config.rs / utils/ / library/ / catalog/ / backup/ / presentation/`

#### 跑全 e2e
- [ ] E1: `cargo test`
- [ ] E2: `cargo run -- help` + `cargo run -- source --help` 等 12 個 subcommand 比對基線（先 git stash 切回 main 跑一遍存 baseline，再切回 refactor 比對）
- [ ] E3: `sqlite3 ~/.local/share/novel-looker/novel-looker.db .schema` 比對
- [ ] E4: `cargo run -- source import examples/gutenberg.json` + `book-sources/uukanshu.json`
- [ ] E5: `cargo run -- config show`
- [ ] E6: `cargo run -- add --source https://uukanshu.cc https://uukanshu.cc/book/21940/`
- [ ] E7: `cargo run -- sync $NID` + `cargo run -- read $NID 0`（章節文字 ≥ 50 行）
- [ ] E8: `cargo run -- backup`
- [ ] E9: `cargo run -- tui $NID`，操作一輪
- [ ] E10: legado-converter 對 yckceo 7321 重跑
- [ ] E11: `git diff --stat .claude/skills/`

#### 整合測試自檢
- [ ] `grep -rn "use rusqlite" src/*/service/ 2>/dev/null`：無輸出
- [ ] `grep -rnE "use crate::[a-z]+::dao" src/*/service/ 2>/dev/null`：無輸出
- [ ] `grep -rnE "use crate::(catalog|library|backup)::facade" src/*/facade.rs 2>/dev/null | grep -v "src/backup/facade.rs"`：無輸出（backup/facade 呼 library/facade 是 Conformist 例外）
- [ ] `grep -rnE "use crate::(catalog|library|backup)::(service|dao)" src/presentation/handlers/`：無輸出
- [ ] `grep -rln "use rusqlite" src/ | grep -vE "(dao)\.rs"`：無輸出
- [ ] `grep -rnE "Connection::open|LibraryDb::open" src/ | grep -vE "src/(main|library/dao)\.rs"`：無輸出（除 main.rs + library/dao.rs 外，其他不開 connection）
- [ ] `grep -rnE "rusqlite|wreq|BookSource|Scraper" src/main.rs`：無輸出（main.rs 不見 domain symbol）
- [ ] `grep -rn "Emulation::Chrome131" src/catalog/service/scraper.rs`：恰 1 行
- [ ] `grep -rnE "_legacy|legacy_" src/ 2>/dev/null`：無輸出（殘留清零）
- [ ] **Shared Kernel 寫入分工**：sync 後 `sqlite3 ~/.local/share/novel-looker/novel-looker.db "SELECT idx,name,length(coalesce(content,'')) FROM chapters WHERE novel_id=$NID ORDER BY idx LIMIT 3"`，content length = 0；read 第 0 章後同 query，第 0 列 content length > 0
- [ ] **TOC re-sync 不破壞 progress (E13)**：先記下 `BEFORE_IDX=$(sqlite3 ... "SELECT chapter_index FROM progress WHERE novel_id=$NID")`；跑 `cargo run -- sync $NID`；確認 `sqlite3 ... "SELECT chapter_index FROM progress WHERE novel_id=$NID"` 與 `$BEFORE_IDX` 相等
- [ ] **連續操作不撞 lock (E14)**：`cargo run -- sync $NID && cargo run -- backup` 不出現 `database is locked`
- [ ] **錯誤 context 品質 (E12)**：`cargo run -- add --source https://uukanshu.cc http://bad.invalid 2>&1 | grep -E "URL|rule"` 至少 1 行

#### Commit + push
- [ ] `git add -A && git status`：確認 `.claude/skills/` 不在 staged 列
- [ ] `git commit -m "refactor: DDD context split (4 bounded context × 5 layer)"`
- [ ] `git push`

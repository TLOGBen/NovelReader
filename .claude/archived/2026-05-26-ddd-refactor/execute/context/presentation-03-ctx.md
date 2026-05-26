Goal: |
  從 goal.md 與此 task 相關片段：
  - 重構為 4 個 bounded context × 5 層架構；本 task 屬 Presentation context。
  - In scope (item)：搬動 src/reader.rs 為 src/presentation/reader.rs。
  - C1 編譯通過、無新 warning（基線 2 條 dead_code）。
  - C3 CLI grammar 完全不變（含 tui subcommand）。
  - C9 service 不直接接觸 SQL（reader.rs 屬 presentation；其對 storage 的呼叫應走 library::facade 而非直接觸 DAO）。

Requirements:
  REQ-001:
    描述: 目錄結構符合 DDD 藍圖；既有檔案按 File-to-Context Mapping 全數搬完，根目錄只剩 main.rs / config.rs / utils 入口。
    Scen_1_3_relevant: |
      Given 重構完成
      When 列出 src/ 第一層 .rs 檔
      Then 只剩 main.rs 與 config.rs；不再有 cli.rs, scraper.rs, storage.rs, backup.rs, **reader.rs**, models.rs
  REQ-004:
    描述: 既有功能行為等價；所有 CLI subcommand 在重構後行為一致；e2e 跑通。
    TUI_subset: TUI 啟動可切章、不 crash、不 panic；對應 E9 場景。

Scenarios:
  - id: REQ-001 Scen 1.3
    text: |
      Given 重構完成
      When 列出 src/ 第一層 .rs 檔
      Then 只剩 main.rs 與 config.rs；不再含 reader.rs
  - id: REQ-004 Scen 4.3 (read 等價)
    text: |
      Given uukanshu 已加入書架
      When 執行 cargo run -- sync N 接著 cargo run -- read N idx
      Then read 印對應章節 title + 內文 ≥ 50 行
  - id: E9 (TUI subset)
    text: |
      Given E7 已 sync
      When 執行 cargo run -- tui $NID
      Then 開啟 ratatui，j/k 換章正常，章節列表非空
  - id: E7b (Cache hit 不重抓)
    text: |
      Given E7 已 read 一次
      When 第二次 RUST_LOG=debug cargo run -- read $NID 0
      Then 不應出現 fetch_chapter_content log（cache hit 不打 HTTP）

Task:
  id: TASK-presentation-03
  title: 搬 reader.rs → presentation/reader.rs
  需求追溯: REQ-001 (Scen 1.3), REQ-004 (TUI 子集), test.md 邊界「Reading session state 暫居 Presentation 不洩」
  目標: |
    src/reader.rs 搬到 src/presentation/reader.rs；功能完全等價（不重構 ratatui 內部）；
    加 doc comment 註明 ReaderApp 含 Reading session state 待 OQ-2 觸發時拆出。
  驗收標準:
    - src/presentation/reader.rs 存在，內容等同舊 reader.rs（最多 import 路徑微調）
    - src/reader.rs 已刪
    - src/main.rs 移除 `mod reader;`
    - cargo run -- tui N：開啟 TUI，j/k 切換章節正常，q 離開（不 crash、不 panic）
    - 對 Library DAO 的呼叫改透過 facade（保持 handler/reader 都不直接呼 dao 的原則）
  步驟:
    搬檔:
      - mv src/reader.rs src/presentation/reader.rs
      - 更新 import：`use crate::storage::Storage` → `use crate::library::dao::LibraryDb`（已於 TASK-presentation-02 完成）
      - 更新 import：`use crate::scraper::Scraper` → `use crate::catalog::service::scraper::Scraper`
      - 內部 `store.get_chapter / save_progress / list_chapters / get_novel / save_chapter_content / get_progress` 改為 `library::facade::*` 呼叫
      - 內部 `store.get_source` 改為 `catalog::facade::get_source`
      - `scraper.fetch_content(...)` 改為 `catalog::facade::fetch_chapter_content(scraper, src, url)`
      - 在檔案頂端加 doc comment（見 Files 段下方 snippet）
    模組宣告:
      - presentation/mod.rs 目前有 `pub mod cli; pub mod handlers;`，需 ADD `pub mod reader;`
      - handlers/tui.rs：`crate::reader::run(&mut ctx.db, novel_id).await` → `crate::presentation::reader::run(&mut ctx.db, novel_id).await`
    更新 main.rs:
      - 移除 `mod reader;`（透過 presentation::reader 可達）
    驗證:
      - LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build → exit 0, ≤ 2 warnings
      - cargo test → 4 個案例通過
      - ls src/reader.rs → ENOENT
      - find src/presentation -name "reader.rs" → 1 match
      - grep "use crate::library::dao\|store.get_novel\|store.list_chapters" src/presentation/reader.rs 大多應為 facade 呼叫
      - grep -rn "scroll_offset\|chapter_index" src/library/ 只在 ReadProgress struct + DAO 序列化處
      - 可選：cargo run -- tui 3 + 手動 j/k/q（若 agent 無終端可略）
      - 可選：第二次 RUST_LOG=debug cargo run -- read $NID 0 不出現 fetch_chapter_content log（E7b）

Design:
  presentation_position:
    - presentation/reader.rs：ReaderApp + event_loop（保留 reading session state pending split for OQ-2）
    - presentation/mod.rs doc：`//! Presentation: CLI + TUI 翻譯人類意圖`
  facade_call_topology:
    - handler 可呼三個 context facade；facade 不互呼（reader 屬 presentation，視同 handler 處理）
    - Catalog → Library 共用 Shared Kernel（sources / chapters TOC writes 由 Catalog 寫；chapters.content 由 Library 寫）
    - cache miss 流程：Library facade get_chapter → 若 cache miss handler 取得 ChapterMeta → 呼 catalog::facade::fetch_chapter_content → 回填 library::facade::save_chapter_content
  borrow_rules:
    - 唯讀（SELECT）：`fn xxx(db: &LibraryDb, ...) -> Result<T>`
    - 單一寫入：`fn xxx(db: &mut LibraryDb, ...)`
    - Transaction：`fn xxx(db: &mut LibraryDb, ...)`
    - reader::run 簽名應沿用 `pub async fn run(store: &mut LibraryDb, novel_id: i64) -> Result<()>`
  error_handling:
    - DAO 用 anyhow::Context 加上下文；service 用 anyhow::bail！；facade 不另包；action/handler 印 `{e:#}`

Test:
  e2e:
    - E1 (cargo test 全綠)：每 step 後跑
    - E9 (TUI 啟動切章)：step 4 後跑（本 task 屬 step 4 範圍）
    - E7b (Cache hit 不重抓)：step 3 後跑，與 read flow 同源
  integration:
    - Handler → 三 context facade（reader 視同 handler，不直接 import dao/service）
    - Catalog/Library DAO 共享 connection（reader 收 &mut LibraryDb 直接借用）
  boundary_conditions:
    - "Reading session state 暫居 Presentation 不洩"：
        grep "scroll_offset\|chapter_index" src/library/ 只在 ReadProgress struct + DAO 序列化處
    - "Cache miss 流程不破壞既有行為 + E7b"：
        第二次同章 read 不出現 fetch_chapter_content log
    - "Cloudflare bypass 保留"：
        grep "Emulation::Chrome131" src/catalog/service/scraper.rs 恰 1 行（與本 task 無直接修改，但 catalog facade 呼叫鏈相依）
    - "TRANSITION 殘留清零"：
        grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/ 無輸出

Constraints:
  env:
    - LIBCLANG_PATH=/usr/lib/llvm-18/lib（cargo build 必要）
  baseline:
    - baseline warnings = 2（dead_code: select_within, extract_all_doc）；本 task 完成後 ≤ 2
  branch: refactor/ddd-context-split
  readonly:
    - 不修改 .claude/analyze/ 任何文件
    - 不修改 .claude/skills/（REQ-006 Scen 6.1）
    - 不動 SQLite schema、CLI grammar、book-source JSON、config.toml key
    - 不重構 ratatui 內部邏輯（只搬檔 + import 路徑微調 + facade 化呼叫）
    - 不抽 Reading bounded context（OQ-2 留延後）
  invariants:
    - "facade 不互呼"通則：reader.rs 屬 presentation 視同 handler 編排層，可呼多個 context 的 facade
    - reader 不可直接 import 任何 service 模組或 rusqlite
    - 修改前 MUST Read 既有檔案（read-before-write 規則）
  doc_comment_required: |
    //! ratatui TUI reader.
    //! ReaderApp 內含 reading session state（current_chapter, scroll, content cache）—
    //! 此屬 Reading session 概念，未來若拆出 Reading bounded context（OQ-2 觸發條件：
    //! annotation / highlight / 多 session）會搬到 reading/ 目錄。目前居此處。

Files:
  new:
    - src/presentation/reader.rs   # 從 src/reader.rs 搬入，加 doc comment + facade 化呼叫
  modify:
    - src/presentation/mod.rs       # ADD `pub mod reader;`（目前只有 `pub mod cli; pub mod handlers;`）
    - src/presentation/handlers/tui.rs  # `crate::reader::run(...)` → `crate::presentation::reader::run(...)`
    - src/main.rs                   # 移除 `mod reader;`
  delete:
    - src/reader.rs
  facade_dependencies:
    - library::facade::{get_novel, list_chapters, get_chapter, save_chapter_content, save_progress, get_progress}
    - catalog::facade::{get_source, fetch_chapter_content}

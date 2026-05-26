# Context for TASK-presentation-02

Goal: |
  按 DDD 藍圖把 novel-looker 從扁平結構重構為 4 個 bounded context (Catalog / Library /
  Backup / Presentation) x 5 層 (action / facade / service / DAO / utils)，不破壞任何
  使用者可見對外介面 (CLI grammar / SQLite schema / book-source JSON / config.toml /
  .claude/skills/ 對 CLI 的相依)。

  TASK-presentation-02 specific scope (the biggest task in the refactor):
  - 在 presentation/mod.rs 建立 AppContext struct，holds LibraryDb + Scraper + Config
  - 把 src/cli.rs::run() 的 match arms 拆成 11 個 handler 檔（加 mod.rs 共 12）
  - 重寫 src/main.rs 一次性 wire AppContext，呼 presentation::cli::run(cli, &mut ctx)
  - 清理 src/storage.rs alias (TRANSITION marker 清零)：grep 全 codebase 把
    `use crate::storage::Storage` 改為 `use crate::library::dao::LibraryDb`
  - 刪除 src/cli.rs 與 src/storage.rs (兩個過渡檔)
  - src/main.rs 移除 `mod cli;` 與 `mod storage;`
  - 完成後所有 e2e (E1-E8, E11，TUI 留下個 task) 通過

  Goal-level Criteria touched by this task:
  - C1 編譯通過 (warning <= baseline 2)
  - C2 所有單元測試通過
  - C3 CLI grammar 100% 不變 (subcommand list, help text byte-identical)
  - C7 e2e (add -> sync -> read) 全流程成功
  - C8 目錄結構符合藍圖 (src/presentation/handlers/ 12 檔)

Requirements:
  REQ-001:
    description: 重構後 src/ 下出現 4 個 bounded context 目錄 + utils/，根目錄只剩
      main.rs / config.rs / utils 入口。
    relevant_scenarios:
      - "Scen 1.3 (舊扁平檔案已搬移)：列出 src/ 第一層 .rs 檔，只剩 main.rs 與
         config.rs；不再有 cli.rs / scraper.rs / storage.rs / backup.rs /
         reader.rs / models.rs"
  REQ-002:
    description: Service 與 DAO 層依賴隔離；service 不 import rusqlite 或 dao；
      facade 是唯一同時呼叫 service 與 DAO 的層。
    relevant_scenarios:
      - "Scen 2.4 (facade 可同時呼 service + DAO)：handler 是 application service /
         跨 context facade 編排，但本 task 對 handler 的約束是：只呼 facade，不直接
         呼 service 或 dao module"
  REQ-003:
    description: CLI grammar / SQLite schema / book-source JSON / config.toml 對外
      介面 100% 不變。
    relevant_scenarios:
      - "Scen 3.1 (CLI subcommand 列表不變)：cargo run -- --help 列出
         source / search / add / shelf / sync / read / tui / config / export /
         import / backup / help 12 個 subcommand，與重構前一致"
  REQ-004:
    description: 既有功能行為等價；所有 CLI subcommand 在重構後行為與重構前一致。
    relevant_scenarios:
      - "Scen 4.1 (cargo test 全綠，source::rule::tests 4 案例通過)"
      - "Scen 4.2 (Cloudflare bypass 仍生效：add --source https://uukanshu.cc
         https://uukanshu.cc/book/21940/ 印 ✓ 加入書架 (#N) 超維術士 / 佚名)"
      - "Scen 4.3 (TOC 同步 + 章節讀取等價：sync N 印 ✓ 同步 N 章；read N idx 印
         章節 title + 內文 >= 50 行)"
      - "Scen 4.4 (備份流程等價)"
      - "Scen 4.5 (舊 backup JSON 可被新 binary 還原)"

Scenarios:
  - id: REQ-001 Scen 1.3
    given: 重構完成
    when: 列出 src/ 第一層 .rs 檔
    then: 只剩 main.rs 與 config.rs；不再有 cli.rs / scraper.rs / storage.rs /
      backup.rs / reader.rs / models.rs (本 task 負責刪 cli.rs + storage.rs)
  - id: REQ-003 Scen 3.1
    given: 重構完成
    when: cargo run -- --help
    then: 輸出列出 source / search / add / shelf / sync / read / tui / config /
      export / import / backup / help 12 個 subcommand
  - id: REQ-004 Scen 4.1
    given: 重構完成
    when: cargo test
    then: 退出碼 0；source::rule::tests 4 案例通過
  - id: REQ-004 Scen 4.2
    given: uukanshu 書源已 import
    when: cargo run -- add --source https://uukanshu.cc https://uukanshu.cc/book/21940/
    then: 輸出 ✓ 加入書架 (#N) 超維術士 / 佚名
  - id: REQ-004 Scen 4.3
    given: 超維術士已在書架
    when: cargo run -- sync $NID && cargo run -- read $NID 0
    then: sync 印 ✓ 同步 N 章；read 印 title + 內文 >= 50 行

Task:
  id: TASK-presentation-02
  title: 拆 run() 為 handlers/*.rs (12 個 subcommand 各一檔)
  prerequisites: library, catalog, backup 三 group 完成；TASK-presentation-01 已建立
    presentation/ 結構與 cli.rs type 定義

  goal: |
    把 src/cli.rs 的 run() match arms 拆成 12 個 handler 函數，分檔到
    src/presentation/handlers/{source, search, add, shelf, sync, read, tui,
    config, export, import, backup}.rs。每個 handler 函數簽名
    `pub async fn handle(args, ctx: &mut AppContext) -> Result<()>`。
    建立 AppContext struct 在 presentation/mod.rs 內，持有
    LibraryDb + Scraper + Config (main.rs 一次性 wire up)。
    同時清理 src/storage.rs alias (TRANSITION marker 清零)。

  acceptance:
    - "src/presentation/handlers/*.rs 共 11 個檔案 (source.rs 處理
       SourceCmd::{Import, List}; config.rs 處理 ConfigCmd::{Show, Set, Path})，
       加 mod.rs 共 12 檔"
    - "每個 handler 內部只呼對應 context 的 facade (不直接呼 service 或 dao)"
    - "presentation/mod.rs 定義 pub struct AppContext { pub db: LibraryDb,
       pub scraper: Scraper, pub config: Config }"
    - "src/cli.rs (root) 刪除"
    - "src/storage.rs (root) 刪除 (TRANSITION 清零)"
    - "src/main.rs 改為一次性 wire AppContext，呼 presentation::cli::run(cli, &mut ctx).await"
    - "src/main.rs 移除 `mod cli;` 與 `mod storage;`"
    - "presentation::cli::run 內部 match arm -> handlers::xxx::handle(args, ctx) dispatch"
    - "所有 e2e (E1-E8, TUI 除外) 通過"
    - "grep -rnE \"use crate::(catalog|library|backup)::(service|dao)\" src/presentation/handlers/ 無輸出"
    - "grep -rnE \"_legacy|legacy_|TRANSITION:|MOVED:\" src/ 無輸出"

  steps:
    1_build_AppContext:
      file: src/presentation/mod.rs
      add: |
        pub struct AppContext {
            pub db: library::dao::LibraryDb,
            pub scraper: catalog::service::scraper::Scraper,
            pub config: crate::config::Config,
        }

        impl AppContext {
            pub fn bootstrap(config: Config) -> Result<Self> {
                let db = LibraryDb::open()?;
                let scraper = Scraper::new()?;
                Ok(Self { db, scraper, config })
            }
        }
    2_rewrite_main:
      file: src/main.rs
      replace_with: |
        mod backup;
        mod catalog;
        mod config;
        mod library;
        mod presentation;
        mod reader;  // still at root, TASK-presentation-03 moves it
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
      removes:
        - "mod cli;"
        - "mod storage;"
    3_cleanup_storage_alias:
      - "grep `use crate::storage::Storage` 全 codebase，每處改為
         `use crate::library::dao::LibraryDb` (含 reader.rs / handlers /
         任何 still-pointing 處)"
      - "rm src/storage.rs"
      - "src/main.rs 移除 `mod storage;` (已包含在 step 2)"
      - "驗證：grep -rnE \"_legacy|legacy_|TRANSITION:|MOVED:\" src/ 無輸出"
    4_split_handlers:
      directory: src/presentation/handlers/
      signature: "pub async fn handle(args, ctx: &mut AppContext) -> Result<()>"
      constraint: 每個 handler 內只呼 facade (catalog::facade::* / library::facade::* /
        backup::facade::* / config::*)；不直接 import service 或 dao 模組
      files:
        - name: source.rs
          handles: "Cmd::Source with sub-enum SourceCmd::{Import, List}"
        - name: search.rs
          handles: Cmd::Search
        - name: add.rs
          handles: Cmd::Add (呼 catalog::facade::fetch_novel_info + library::facade::add_novel)
        - name: shelf.rs
          handles: Cmd::Shelf
          example: 呼 library::facade::list_shelf(&ctx.db)，印出書架列表
        - name: sync.rs
          handles: Cmd::Sync
          example: 呼 library::facade::get_novel + catalog::dao::get_source +
            catalog::facade::sync_toc(&mut ctx.db, &ctx.scraper, ...)
        - name: read.rs
          handles: Cmd::Read
        - name: tui.rs
          handles: Cmd::Tui
          note: "delegates to crate::reader::run(&mut ctx.db, novel_id).await — reader 仍
            在 root，TASK-presentation-03 才搬到 presentation/reader.rs"
        - name: config.rs
          handles: "Cmd::Config with sub-enum ConfigCmd::{Show, Set, Path}"
        - name: export.rs
          handles: Cmd::Export
        - name: import.rs
          handles: Cmd::Import
        - name: backup.rs
          handles: Cmd::Backup
          example: 呼 crate::backup::facade::run_backup(&ctx.db, &ctx.config)
        - name: mod.rs
          declares: "pub mod source; pub mod search; pub mod add; ... pub mod backup;"
    5_rewrite_cli_run:
      file: src/presentation/cli.rs
      add: |
        pub async fn run(cli: Cli, ctx: &mut AppContext) -> Result<()> {
            match cli.cmd {
                Cmd::Source(sc) => handlers::source::handle(sc, ctx).await,
                Cmd::Search { .. } => handlers::search::handle(..., ctx).await,
                // ... 全部 11 個 dispatch
            }
        }
    6_remove_old_cli:
      - "rm src/cli.rs"
    7_verify:
      - "cargo build (exit 0, warnings <= 2)"
      - "cargo run -- --help 與 baseline /tmp/help-baseline.txt 完全一致"
      - "for cmd in source search add shelf sync read tui config export import backup;
         do diff <(./target/debug/novel-looker $cmd --help) /tmp/help-$cmd-baseline.txt;
         done — 全部空 diff"
      - "cargo test (4 pass)"
      - "cargo run -- shelf (2 books)"
      - "cargo run -- source list (2 sources)"
      - "LIBCLANG_PATH=/usr/lib/llvm-18/lib NID=$NID cargo run -- sync $NID"
      - "grep -rnE \"use crate::(catalog|library|backup)::(service|dao)\"
         src/presentation/handlers/  # 無輸出"
      - "grep -rnE \"_legacy|legacy_|TRANSITION:|MOVED:\" src/  # 無輸出"
      - "ls src/cli.rs  # ENOENT"
      - "ls src/storage.rs  # ENOENT"
      - "grep -E \"^mod (cli|storage);\" src/main.rs  # 無輸出"

Design:
  AppContext_shape: |
    pub struct AppContext {
        pub db: library::dao::LibraryDb,
        pub scraper: catalog::service::scraper::Scraper,
        pub config: crate::config::Config,
    }
    impl AppContext {
        pub fn bootstrap(config: Config) -> Result<Self> {
            let db = LibraryDb::open()?;
            let scraper = Scraper::new()?;
            Ok(Self { db, scraper, config })
        }
    }
    # by value held in AppContext. Handler 統一收 `&mut AppContext` (即使該 use case
    # 只 read，也用 &mut 給統一性，避免 dispatcher match arm 內混 & vs &mut)

  LibraryDb_interface: |
    pub struct LibraryDb { conn: Connection }
    impl LibraryDb {
        pub fn open() -> Result<Self> { ... }
        pub fn conn(&self) -> &Connection { &self.conn }
        pub fn conn_mut(&mut self) -> &mut Connection { &mut self.conn }
    }

  borrow_rules:
    SELECT_readonly: "fn xxx(db: &LibraryDb, ...) -> Result<T>"
    single_write: "fn xxx(db: &mut LibraryDb, ...) -> Result<T>"
    transaction: "fn xxx(db: &mut LibraryDb, ...) -> Result<T>"
    Catalog_DAO_同規則: "catalog::dao::replace_toc(&mut LibraryDb, ...) /
      catalog::dao::list_sources(&LibraryDb)"

  cross_context_topology: |
    handlers (Presentation) 是唯一跨 context 編排點，可呼三個 context 的 facade
    (Catalog / Library / Backup)
    facade 不互呼 (避免 cycle)，例外：backup::facade 可呼 library::facade
    (Conformist 關係的合法表達)

  call_examples:
    shelf_handler: 呼 library::facade::list_shelf(&ctx.db)
    sync_handler: 呼 library::facade::get_novel + catalog::dao::get_source +
      catalog::facade::sync_toc(&mut ctx.db, &ctx.scraper, ...)
    backup_handler: 呼 crate::backup::facade::run_backup(&ctx.db, &ctx.config)
    tui_handler: 呼 crate::reader::run(&mut ctx.db, novel_id).await  # reader 仍在 root

  use_case_flow_add_sync_read: |
    參考 design.md sequence diagram：
    - add: handler -> catalog::facade::fetch_novel_info -> library::facade::add_novel
      -> library DAO upsert_novel
    - sync: handler -> library::facade::get_novel -> catalog::facade::fetch_toc
      -> library::facade::replace_toc -> library DAO DELETE + INSERT chapters (txn)
    - read: handler -> library::facade::get_chapter (cache hit/miss) -> if miss:
      catalog::facade::fetch_chapter_content -> library::facade::save_chapter_content

  Shared_Kernel_note: |
    catalog/dao.rs 與 library/dao.rs 開頭加：
    //! NOTE: Shared Kernel — sources.* 與 chapters.{idx,name,url} 由 Catalog 寫；
    //! chapters.content 由 Library 寫。修改任一方 schema 需同步檢視對方 DAO。

  TRANSITION_marker_convention: |
    refactor 過程中所有暫時別名、過渡 re-export、暫留檔案，強制標
    `// TRANSITION:` 註解 (含拆完移除的 task 編號)。例：
    // TRANSITION: removed in task-presentation-02 cleanup
    pub use crate::library::dao::LibraryDb as Storage;
    最終 PR 前 grep 必須為空。

Test:
  unit_tests:
    - "cargo test 全綠；source::rule::tests 4 案例通過 (parse_basic /
       parse_attr_and_replace / parse_alternatives / extract_text_with_fallback)"

  e2e_for_this_task:
    - "E1 編譯 + 單元測試：cargo build (warning <= 2) + cargo test"
    - "E2 CLI grammar diff：novel-looker help + 11 個 subcommand --help 與
       /tmp/help-*-baseline.txt byte-identical (中間態跑 step 4 後)"
    - "E3 SQLite schema diff (每 step 後)"
    - "E4 既有書源 import (gutenberg + uukanshu)"
    - "E5 config.toml round-trip"
    - "E6 uukanshu add (Cloudflare bypass)：印 ✓ 加入書架 (#N) 超維術士 / 佚名"
    - "E7 sync $NID + read $NID 0 (內文 >= 50 行)"
    - "E7b Cache hit 不重抓：第二次 RUST_LOG=debug cargo run -- read $NID 0 2>&1
       | grep fetch_chapter_content 應無輸出"
    - "E8 backup (印 ✓ 備份 N 本書 [local] -> ...)"
    - "E9 TUI 留 TASK-presentation-03 後跑"
    - "E13 TOC re-sync 不破壞 progress"
    - "E14 連續操作不撞 lock (cargo run -- sync $NID && cargo run -- backup
       不出 database is locked)"

  integration_self_check_greps:
    - "grep -rn \"use rusqlite\" src/*/service/ 2>/dev/null  # 無輸出"
    - "grep -rnE \"use crate::[a-z]+::dao\" src/*/service/ 2>/dev/null  # 無輸出"
    - "grep -rnE \"use crate::(catalog|library|backup)::(service|dao)\"
       src/presentation/handlers/  # 無輸出 (handler 只呼 facade)"
    - "grep -rln \"use rusqlite\" src/ | grep -vE \"(dao)\\.rs\"  # 無輸出"
    - "grep -rnE \"Connection::open|LibraryDb::open\" src/ |
       grep -vE \"src/(main|library/dao)\\.rs\"  # 無輸出"
    - "grep -rnE \"rusqlite|wreq|BookSource|Scraper\" src/main.rs  # 無輸出"
    - "grep -rn \"Emulation::Chrome131\" src/catalog/service/scraper.rs  # 恰 1 行"
    - "grep -rnE \"_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:\" src/
       2>/dev/null  # 無輸出"

Constraints:
  environment:
    - "LIBCLANG_PATH=/usr/lib/llvm-18/lib 每個 shell session 必設 (wreq -> boring-sys2
       -> bindgen -> libclang 編譯鏈缺此會 fatal link error)"
    - "branch: refactor/ddd-context-split (不在 main 上直接 refactor)"
    - "不執行 cargo clean (首次 wreq+boring-sys2 編譯 ~2-3 分鐘，重複 clean 會把
       pipeline 拉長到不可接受)"

  read_only:
    - ".claude/skills/ 三個 skill (parse-novel-site / legado-converter / add-to-shelf)
       一字未動；git diff --stat .claude/skills/ 必須無輸出"
    - ".claude/wip/ + .claude/analyze/ 重構期間不得寫入或 commit"
    - "SQLite schema (sources / novels / chapters / progress 四張表) 完全不變"
    - "CLI grammar (subcommand list / flag / help text) 逐字不變"
    - "book-source JSON 欄位 (含 camelCase serde rename) 不變"
    - "config.toml key (backup.backend / backup.keep / backup.local.path /
       backup.webdav.*) 不變"

  baseline_warnings: 2  # dead_code: select_within, extract_all_doc
    # 重構後 warning 數量必須 <= 2

  architectural:
    - "handler 內只呼 facade (catalog::facade::* / library::facade::* /
       backup::facade::* / config::*)；不直接 import service 或 dao 模組"
    - "facade 不互呼跨 context (例外：backup::facade 可呼 library::facade，
       Conformist 關係)"
    - "AppContext by value held in main 的 stack；handler 統一收 &mut AppContext
       (即使 read-only use case 也用 &mut，給統一性)"
    - "DB connection 共享：rusqlite 預設 single-threaded mode；多 Connection 對同
       DB 檔在序列操作下會出現 SQLITE_BUSY (E14 驗證)；main.rs 一次性 open LibraryDb
       由 AppContext 持有"
    - "main.rs 純 wiring，不見任何 domain symbol (rusqlite / wreq / BookSource /
       Scraper 都不出現)"
    - "Cloudflare bypass：Emulation::Chrome131 在 src/catalog/service/scraper.rs 恰 1 行"
    - "TRANSITION 清零：完成後 grep -rnE '_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:'
       src/ 無輸出"
    - "刪除 src/cli.rs 與 src/storage.rs；main.rs 移除 mod cli; 與 mod storage;"
    - "reader 仍在 root (mod reader;)，下個 task TASK-presentation-03 才搬到
       presentation/reader.rs"

  out_of_scope:
    - "不改 SQLite schema / CLI grammar / config.toml key / book-source JSON 欄位"
    - "不修改 .claude/skills/ 任何檔案"
    - "不新增 trait NovelRepository 之類抽象 (KD 7：直接 import，避免過早抽象)"
    - "不抽出 Reading bounded context (OQ-2 觸發條件未滿足)"
    - "不搬 reader.rs (下個 task 做)"
    - "不改善 reader.rs 同步 fetch 阻塞 UI"

Files:
  create:
    - src/presentation/handlers/source.rs   # SourceCmd::{Import, List}
    - src/presentation/handlers/search.rs   # Cmd::Search
    - src/presentation/handlers/add.rs      # Cmd::Add
    - src/presentation/handlers/shelf.rs    # Cmd::Shelf
    - src/presentation/handlers/sync.rs     # Cmd::Sync
    - src/presentation/handlers/read.rs     # Cmd::Read
    - src/presentation/handlers/tui.rs      # Cmd::Tui (delegates to crate::reader::run)
    - src/presentation/handlers/config.rs   # ConfigCmd::{Show, Set, Path}
    - src/presentation/handlers/export.rs   # Cmd::Export
    - src/presentation/handlers/import.rs   # Cmd::Import
    - src/presentation/handlers/backup.rs   # Cmd::Backup (呼 backup::facade::run_backup)

  modify:
    - src/presentation/mod.rs:
        action: add AppContext struct + impl AppContext { pub fn bootstrap(config) }
        also: 加 pub mod cli; pub mod handlers; (若尚未 pub) — handlers/mod.rs
          declares 全部 11 個 handler module
    - src/presentation/handlers/mod.rs:
        action: declare "pub mod source; pub mod search; pub mod add; pub mod shelf;
          pub mod sync; pub mod read; pub mod tui; pub mod config; pub mod export;
          pub mod import; pub mod backup;"
    - src/presentation/cli.rs:
        action: 加 "pub async fn run(cli: Cli, ctx: &mut AppContext) -> Result<()>"
          函數，內含 match cli.cmd → dispatch handlers::xxx::handle
    - src/main.rs:
        action: 完全重寫成 wiring 殼，移除 mod cli; 與 mod storage;，加 mod presentation;
          (若已有可略)；body 走 AppContext::bootstrap + presentation::cli::run

  cleanup_storage_alias_modify: |
    grep 全 codebase，凡有 `use crate::storage::Storage` 處皆改為
    `use crate::library::dao::LibraryDb`。預期影響：
    - src/reader.rs (root，下個 task 才搬)
    - 各 handlers 內任何 still-pointing alias
    - 任何其他 module 殘留

  delete:
    - src/cli.rs      # run() match arms 已搬到 presentation/cli.rs::run + handlers/
    - src/storage.rs  # TRANSITION alias 已被全 codebase 改用 library::dao::LibraryDb

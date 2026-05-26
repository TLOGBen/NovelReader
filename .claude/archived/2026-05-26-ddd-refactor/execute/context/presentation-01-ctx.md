# Context: TASK-presentation-01

```yaml
Goal: |
  Per goal.md：把 novel-looker 從按技術分類的扁平結構（cli.rs, scraper.rs,
  storage.rs, backup.rs, reader.rs, source/, config.rs, models.rs, main.rs）
  重構為按 4 個 bounded context（Catalog / Library / Backup / Presentation）×
  5 層架構（action / facade / service / DAO / utils）組織的目錄結構，且
  不破壞任何使用者可見的對外介面（CLI grammar、SQLite schema、book-source
  JSON 格式、config.toml key、.claude/skills/ 對 CLI 的相依）。

  本 task 對應目標的子集：
  - C1 編譯通過、無新 warning（基線 = 2 條 dead_code）
  - C3 CLI grammar 完全不變（help 文字逐字一致）
  - C8 目錄結構符合 DDD 藍圖（presentation/ context 目錄建立）

Requirements:
  REQ-001: |
    目錄結構符合 DDD 藍圖
    描述：重構後 src/ 下出現 4 個 bounded context 目錄（catalog/ library/
    backup/ presentation/）+ utils/，每個 context 內按需要含 dao.rs、
    service/、facade.rs；既有檔案按 .claude/wip/ddd-analysis.md §4
    File-to-Context Mapping 全數搬完，根目錄只剩 main.rs / config.rs /
    utils 入口。
    （本 task 範圍：建立 presentation/ 目錄骨架；其他 context 已在前置
    group 完成；最後根目錄清空在 TASK-presentation-02/04）

  REQ-003: |
    對外介面 100% 不變
    描述：CLI grammar（subcommand / flag / help text）、SQLite schema、
    book-source JSON 欄位、config.toml key 五項使用者可見契約逐字不變。
    （本 task 觸發：搬 clap type 定義不得改 derive 或 help text 任何字元）

  REQ-005: |
    編譯品質不退化
    描述：重構後編譯不出現新 warning；既有 warning（如 select_within
    dead_code）數量不增加。

Scenarios:
  Scenario_1.1_目錄結構檢查: |
    Given 重構完成的 codebase
    When 執行 `find src -type d -mindepth 1 -maxdepth 1 | sort`
    Then 輸出包含 src/catalog, src/library, src/backup, src/presentation,
         src/utils 五個目錄
    And 不包含 src/source（已搬入 catalog）

  Scenario_1.2_Context_mod_rs_存在且註解職責: |
    Given 4 個 context 目錄都建立
    When Read src/presentation/mod.rs
    Then 檔案最上方有 //! doc comment 描述該 context 的 Purpose 與對外 PL

  Scenario_1.3_舊扁平檔案已搬移: |
    Given 重構完成
    When 列出 src/ 第一層 .rs 檔
    Then 只剩 main.rs 與 config.rs（utils 是目錄）
    （本 task 階段：暫保留 src/cli.rs 作為 re-export shim + run()
     函數宿主，由 TASK-presentation-02 刪除）

  Scenario_3.1_CLI_subcommand_列表不變: |
    Given 重構完成
    When 執行 `cargo run -- --help`
    Then 輸出列出 source / search / add / shelf / sync / read / tui /
         config / export / import / backup / help 12 個 subcommand
         （與重構前一致）

  Scenario_5.1_編譯_warning_不增加: |
    Given 重構前 cargo build 產生 N 條 warning（基線：2 條 dead_code）
    When 重構後 LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker
    Then warning 數量 ≤ N
    And 無 unused_imports、no_use 之類 module 搬移留下的痕跡

  Scenario_5.2_無_cargo_check_error: |
    Given 重構任一階段完成（每個 step 後）
    When 執行 `cargo check`
    Then 退出碼為 0

Task:
  id: TASK-presentation-01
  title: 建立 presentation/ 結構 + 搬 cli.rs 的 type 定義
  目標: |
    src/presentation/{mod.rs, cli.rs, handlers/mod.rs} 建立；
    Cli / Cmd / SourceCmd / ConfigCmd clap struct + enum 從 src/cli.rs
    搬到 src/presentation/cli.rs；舊 src/cli.rs 的 run() 函數暫留在原處，
    下個 task (TASK-presentation-02) 拆。

  驗收標準:
    - src/presentation/{mod.rs, cli.rs} + src/presentation/handlers/mod.rs 建立
    - presentation/mod.rs doc：`//! Presentation: CLI + TUI 翻譯人類意圖。對 plugin layer 的 PL = CLI subcommand grammar。`
    - presentation/cli.rs 含 Cli / Cmd / SourceCmd / ConfigCmd 完整定義（含所有 clap derive、help text）
    - src/main.rs 加 `mod presentation;`（cli mod 暫保留指向 src/cli.rs）
    - cargo build 通過、cargo run -- --help 輸出與重構前逐字一致

  步驟:
    建立_presentation_模組:
      - mkdir -p src/presentation/handlers
      - 'src/presentation/mod.rs：doc + `pub mod cli; pub mod handlers;`（reader 留下 task 搬，故本 task 不宣告 pub mod reader）'
      - 'src/presentation/handlers/mod.rs：佔位（空檔或單一 doc comment）'
      - 'src/main.rs：加 `mod presentation;`（保留 `mod cli;` 直到 TASK-presentation-02）'

    搬_type_定義:
      - 從 src/cli.rs 剪 Cli / Cmd / SourceCmd / ConfigCmd 所有 struct + enum + derive
      - 貼到 src/presentation/cli.rs，含 use clap::* 等 imports
      - '暫不搬 `pub async fn run(cli: Cli)`，下個 task 拆 handlers'
      - 'src/cli.rs 暫保留 run() 函數 + `pub use crate::presentation::cli::{Cli, Cmd, SourceCmd, ConfigCmd};` re-export，讓 src/main.rs 既有 `cli::Cli::parse()` / `cli::run(cli)` 不破壞'

    驗證:
      - cargo build (LIBCLANG_PATH 必設)
      - 'cargo run -- --help：subcommand 列表與 help text 與重構前一致（diff 比對 /tmp/help-baseline.txt）'
      - 每個 subcommand：cargo run -- <cmd> --help 比對 /tmp/help-<cmd>-baseline.txt

Design:
  presentation_目錄結構: |
    src/presentation/             # Presentation bounded context
    ├── mod.rs                    # //! Presentation: CLI + TUI 翻譯人類意圖
    ├── cli.rs                    # Cli/Cmd/SubCmd struct + run() dispatcher
    ├── reader.rs                 # ReaderApp + event_loop（TASK-presentation-03 搬）
    └── handlers/
        ├── mod.rs
        ├── source.rs ...         # TASK-presentation-02 拆

  本_task_只動: |
    - src/presentation/mod.rs（新建，doc + pub mod cli; pub mod handlers;）
    - src/presentation/cli.rs（新建，承接 clap type 定義）
    - src/presentation/handlers/mod.rs（新建，placeholder）
    - src/cli.rs（保留 run()，加 re-export shim）
    - src/main.rs（加 mod presentation;）

  Wiring_暫態: |
    main.rs 仍走 `cli::Cli::parse()` + `cli::run(cli)`。
    cli.rs 變成 thin shim：re-export presentation::cli 的 type +
    暫留 run()。TASK-presentation-02 才會把 main.rs 改為直接從
    presentation 取 type、刪 cli.rs。

  錯誤處理策略: |
    維持現有 anyhow-based 結構。本 task 純搬檔，無新錯誤分類。

Test:
  本_task_驗證項_對應_test_md:
    E1_編譯_單元測試: cargo check + cargo build --bin novel-looker（warning ≤ 2）+ cargo test 全綠
    E2_CLI_grammar_diff: |
      cargo run -- --help vs /tmp/help-baseline.txt：byte-identical
      每個 subcommand：cargo run -- <cmd> --help vs /tmp/help-<cmd>-baseline.txt
    Scenario_5_1_zero_new_warning: |
      基線 cargo build warning 數量 = 2（dead_code: select_within, extract_all_doc）
      重構後 ≤ 2；不得出現 unused_imports

  關鍵邊界條件_本_task_適用:
    - 編譯零 new warning（REQ-005 Scen 5.1）
    - Mod 路徑變動不留 dead use（cargo build 不出現 unused_imports）
    - CLI grammar byte-identical（help 輸出逐字一致）
    - 中間態可編譯（每個 step 後 cargo check 必過）

Constraints:
  env:
    - LIBCLANG_PATH=/usr/lib/llvm-18/lib（cargo build 必設，否則 boring/wreq 撞 bindgen libclang）
    - branch: refactor/ddd-context-split

  唯讀禁忌:
    - .claude/skills/ 一字未動（REQ-006 Scen 6.1，git diff --stat 必須空）
    - .claude/analyze/、.claude/wip/ 不動
    - 不改 SQLite schema、不改 book-source JSON 欄位、不改 config.toml key

  CLI_grammar: |
    100% 不變，help 文字逐字一致（搬 clap derive + help text 不得改一字）

  warning_基線: 2 條 dead_code（select_within, extract_all_doc）；本 task 後 ≤ 2

  本_task_暫態約束:
    - src/cli.rs 暫保留：含 run() + `pub use crate::presentation::cli::*;`（type re-export shim）
    - src/main.rs 暫保留 `mod cli;`（TASK-presentation-02 才刪）
    - src/presentation/mod.rs 本 task 不宣告 `pub mod reader;`（reader 在 TASK-presentation-03 才搬）
    - 不搬 run() 函數本體（TASK-presentation-02 拆 handlers 時才動）
    - 不動 src/reader.rs / backup/ / catalog/ / library/ / utils/ / config.rs
    - re-export shim 上加 `// TRANSITION: removed in task-presentation-02 cleanup`

  不可違反_DDD_藍圖:
    - presentation/cli.rs 是 Presentation context 的 PL hub
    - 本 task 不引入 handler 拆分（下個 task）
    - 本 task 不改 facade/service/dao（前置 group 已完成）

Files:
  to_create:
    - src/presentation/mod.rs            # doc comment + pub mod cli; pub mod handlers;
    - src/presentation/cli.rs            # Cli / Cmd / SourceCmd / ConfigCmd 完整定義 + clap imports
    - src/presentation/handlers/mod.rs   # placeholder（空或單一 doc）

  to_modify:
    - src/cli.rs                          # 移除 type 定義；加 `pub use crate::presentation::cli::{Cli, Cmd, SourceCmd, ConfigCmd};` + TRANSITION marker；保留 pub async fn run()
    - src/main.rs                         # 加 `mod presentation;`（mod cli; 暫留）

  not_touched:
    - src/reader.rs
    - src/backup/
    - src/catalog/
    - src/library/
    - src/utils/
    - src/config.rs
    - .claude/skills/
    - .claude/analyze/
    - book-sources/
    - examples/

  baseline_artifacts:
    - /tmp/help-baseline.txt              # cargo run -- --help 重構前 snapshot
    - /tmp/help-<cmd>-baseline.txt        # 每 subcommand 重構前 snapshot（source/search/add/shelf/sync/read/tui/config/export/import/backup）
```

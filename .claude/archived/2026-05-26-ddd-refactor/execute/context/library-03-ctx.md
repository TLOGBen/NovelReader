---
task_id: TASK-library-03
group: library
generated_for: impl-agent

Goal: |
  按 DDD 藍圖，把 novel-looker 從扁平結構重構為 4 個 bounded context × 5 層架構。
  本 task 對應 Library context 的 facade + service 骨架建立：
  - library/facade.rs 提供 use case 入口（thin wrapper over DAO）
  - library/service/shelf.rs 建立但內容暫空（mod 宣告 + doc comment）

  相關交付條件：
  - C1 編譯通過（warning ≤ 2 baseline）
  - C2 cargo test 全綠
  - C8 目錄結構：library/{mod.rs, dao.rs, facade.rs, service/} 完整
  - C9 service 層不直接接觸 SQL（不 import rusqlite，不 import dao module）

Requirements:
  REQ-001:
    desc: 目錄結構符合 DDD 藍圖
    relevant_scenarios:
      - Scen 1.2 (Context mod.rs 存在且註解職責)：每個 context 目錄頂部 //! doc
        comment 描述職責；library/service/shelf.rs 屬此範疇
  REQ-002:
    desc: Service 層與 DAO 層的依賴隔離
    relevant_scenarios:
      - Scen 2.4 (facade 可同時呼 service + DAO)：facade.rs import 自己 context
        的 service 與 dao 模組（允許）；不直接 import 其他 context 的 service/dao。
        本 task facade 暫時只 delegate 到 dao，未來可組合 service 結果。

Scenarios:
  - id: 1.2
    given: 4 個 context 目錄都建立
    when: Read src/library/mod.rs / src/library/service/shelf.rs
    then: 每個檔案最上方有 //! doc comment 描述職責 / 待實作 invariant
  - id: 2.4
    given: 重構完成
    when: Read src/library/facade.rs
    then: 該檔 import 自 library/dao（允許）；不 import 其他 context 的 service/dao

Task:
  id: TASK-library-03
  title: facade + service 骨架（薄包裝）
  goal: |
    library/facade.rs 提供 use case 入口（thin wrapper over LibraryDb 方法）；
    library/service/shelf.rs 建立但內容暫空（mod 宣告 + doc comment）。
    不擅自抽出未經要求的業務邏輯（invariant 留 OQ-2 trigger）。

  acceptance:
    - library/facade.rs 提供以下 thin wrapper：
        add_novel(db, novel), list_shelf(db), get_novel(db, id),
        get_chapter(db, id, idx), save_chapter_content(db, id, idx, content),
        replace_toc(db, id, chapters), save_progress(db, progress),
        get_progress(db, id), list_chapters(db, id)
    - facade 函數只呼 DAO，不直接寫 SQL
    - library/service/shelf.rs 存在，僅含 doc comment placeholder
    - cargo build + test 通過

  steps:
    write_facade:
      - 對每個 LibraryDb DAO 方法寫對應 facade 函數
      - 第一參數為 &LibraryDb 或 &mut LibraryDb（按 Borrow 規則）
      - 內部直接 delegate 給 db.method(...)
      - 不引入新邏輯（純 pass-through）
      - 加 doc comment：每個 facade fn 一句說明對應的 CLI subcommand
    build_service_skeleton:
      - 建立 src/library/service/shelf.rs，內容：
          //! Library service: pure domain logic / invariants.
          //! 目前無 invariant 需要從別處抽出；待 annotation / multi-session 等
          //! 功能落地時，TOC↔progress 一致性檢查可在此實作（OQ-2 trigger）。
      - 確認 src/library/service/mod.rs 含 `pub mod shelf;`
    verify:
      - cargo build（warning ≤ 2）
      - cargo test（4 個 source::rule::tests 通過）
      - grep -rn "use rusqlite" src/library/service/ → 無輸出
      - grep -rnE "use crate::[a-z]+::dao" src/library/service/ → 無輸出

Design:
  facade_signatures: |
    // 按 design.md「Borrow 規則」：唯讀 = &LibraryDb；寫入/Transaction = &mut LibraryDb
    pub fn add_novel(db: &mut LibraryDb, n: &Novel) -> Result<i64>          // CLI: add
    pub fn list_shelf(db: &LibraryDb) -> Result<Vec<Novel>>                 // CLI: shelf
    pub fn get_novel(db: &LibraryDb, id: i64) -> Result<Option<Novel>>      // CLI: read/tui/sync 前置
    pub fn replace_toc(db: &mut LibraryDb, id: i64, chapters: &[ChapterMeta]) -> Result<()>  // CLI: sync
    pub fn list_chapters(db: &LibraryDb, id: i64) -> Result<Vec<ChapterMeta>>  // CLI: tui 章節列表
    pub fn get_chapter(db: &LibraryDb, id: i64, idx: i64) -> Result<Option<Chapter>>  // CLI: read
    pub fn save_chapter_content(db: &mut LibraryDb, id: i64, idx: i64, content: &str) -> Result<()>  // CLI: read (cache fill)
    pub fn save_progress(db: &mut LibraryDb, p: &ReadProgress) -> Result<()>  // CLI: read/tui quit
    pub fn get_progress(db: &LibraryDb, id: i64) -> Result<Option<ReadProgress>>  // CLI: tui 恢復位置

  facade_topology: |
    handler 是唯一跨 context 編排點；facade 不互呼（backup→library 為 Conformist 例外）。
    本 task facade 只 delegate library/dao 的 LibraryDb method，未呼 service（service 暫空）。

  type_placement_relevant: |
    Novel / ChapterMeta / Chapter / ReadProgress 已於 TASK-library-01 搬入 library/mod.rs（pub re-export）。
    facade.rs 直接 `use super::{Novel, ChapterMeta, Chapter, ReadProgress};` 或 `use crate::library::*;`
    LibraryDb 來自 super::dao（同 context internal）。

  service_skeleton_intent: |
    shelf.rs 是 invariant 邏輯預留位（TOC↔progress 一致性、ChapterCache↔TOC entry 對應）。
    本次不抽出任何邏輯：原碼中既有 invariant 已在 dao.rs::replace_toc transaction 內隱含維持。
    抽出時機（OQ-2 trigger）：annotation / multi-session 功能落地，需要顯式 cross-aggregate validation 時。

  invariant_enforcement_deferred: |
    design.md 註明本次不抽 Reading context (OQ-2)；不加 invariant enforcement，
    避免「為形式而抽象」。service/shelf.rs 留 placeholder 只為符合目錄結構藍圖。

Test:
  strategy: |
    Pure refactor + 結構自檢，不新增 feature 測試。本 task 屬 Step 3 (Library) 中段。

  relevant_e2e_subset:
    - E1: cargo test 全綠（必跑）— 4 個 source::rule::tests
    - E3: SQLite schema 不變（本 task 無 schema 變動，沿用 TASK-library-02 baseline）

  relevant_integration_checks:
    - facade.rs import 自己 context 的 service 與 dao（允許）；不 import 其他 context
    - service/* 不 import rusqlite，不 import 任何 dao module

  edge_cases_to_cover:
    - REQ-002 Scen 2.1: grep -rn "use rusqlite" src/library/service/ → 無輸出
    - REQ-002 Scen 2.2: grep -rnE "use crate::[a-z]+::dao" src/library/service/ → 無輸出
    - REQ-005 Scen 5.1: 編譯 warning ≤ 2 baseline (dead_code: select_within, extract_all_doc)
    - REQ-005 Scen 5.2: cargo check exit code 0

  facade_doc_quality:
    每個 facade fn 加單行 doc comment 標記對應 CLI subcommand（read/sync/shelf 等）。

Constraints:
  authority_invariants:
    - LIBCLANG_PATH=/usr/lib/llvm-18/lib 為編譯前綴（每次 cargo build）
    - Branch: refactor/ddd-context-split
    - .claude/skills/ 絕對不可修改（REQ-006 Scen 6.1：git diff --stat 必須為空）
    - .claude/analyze/ 為 spec authority，唯讀
    - Read-before-write 紀律：Edit/Write 前 MUST Read 該檔

  scope_forbidden:
    - 不引入 trait NovelRepository 抽象（KD 7：避免過早抽象）
    - 不修改 SQLite schema（schema 由 TASK-library-02 已固化）
    - 不修改 CLI grammar / config.toml key / book-source JSON 欄位
    - 不在 service/shelf.rs 中抽出任何 invariant 邏輯（留 OQ-2 trigger）
    - 不引入新 error type（保持 anyhow）
    - facade 函數不可直接寫 SQL（必須 delegate LibraryDb method）
    - facade.rs 不可 import 其他 context 的 dao 或 service（只能 import 自家 dao/service 或其他 context 的 facade — 本 task 不需要跨 context import）

  borrow_rules_strict:
    - 唯讀 (SELECT)：fn xxx(db: &LibraryDb, ...) -> Result<T>
    - 單一寫入 (INSERT/UPDATE)：fn xxx(db: &mut LibraryDb, ...) -> Result<T>
    - Transaction (含多步寫入)：fn xxx(db: &mut LibraryDb, ...) -> Result<T>
    - replace_toc 必須為 &mut LibraryDb（內部 transaction）

  scope_boundaries:
    - sources 表相關 facade（save_source / list_sources / get_source）不在本 task 範圍 —
      屬 Catalog context，將於 TASK-catalog-03 建立 catalog::facade::*
    - storage.rs 仍為 TRANSITION 別名（pub use crate::library::dao::LibraryDb as Storage），
      於 TASK-presentation-02 cleanup 移除

  warning_budget:
    - baseline: 2 條 dead_code (select_within, extract_all_doc)
    - 本 task 後：≤ 2（含；不允許 unused_imports / module 搬移痕跡）

  intermediate_state_invariant:
    - 每步後 cargo check / cargo build / cargo test 三項皆過
    - 完整 PR 前：grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/
      須為空 — 本 task 不可新增 TRANSITION marker（只填空 facade 與 service placeholder）

  coordination:
    - 本 task 為 library group 最後一個 task
    - 下一 group: catalog（TASK-catalog-01 起手）— 依賴 library 全數完成
    - 完成後 storage.rs 仍保留 TRANSITION 別名，等 catalog/backup/presentation 全切換完
      於 TASK-presentation-02 cleanup 移除

Files:
  create:
    - src/library/facade.rs   # 填入 9 個 thin wrapper 函數（覆蓋 TASK-library-01 留下的佔位內容）
    - src/library/service/shelf.rs   # 新建：僅含 doc comment placeholder
  modify:
    - src/library/service/mod.rs   # 確認含 `pub mod shelf;`（若 TASK-library-01 已加則不動）
  read_for_context:
    - src/library/dao.rs   # 確認 LibraryDb 介面與 9 個 method 簽名
    - src/library/mod.rs   # 確認 Novel / ChapterMeta / Chapter / ReadProgress 可見性
    - src/storage.rs       # TRANSITION 別名（不動）
  do_not_touch:
    - .claude/skills/**
    - .claude/analyze/**
    - src/catalog/** (此時還未建立)
    - src/backup.rs (legacy；下個 group 處理)
    - src/cli.rs / src/main.rs (presentation group 處理)
    - SQL schema / migration files
---

Goal: |
  按 DDD 藍圖把 novel-looker 從扁平結構重構為 4 個 bounded context × 5 層架構。
  本 task 是 catalog group 的最後一步：把 sources 表 CRUD + chapters TOC writes 從
  暫存於 library/dao.rs 的位置搬到 catalog/dao.rs（Shared Kernel — 共用 LibraryDb
  Connection），並建立 catalog/facade.rs 作為對外 entry point（search /
  fetch_novel_info / sync_toc / fetch_chapter_content）。
  不破壞 CLI grammar、SQLite schema、book-source JSON、config.toml key、
  .claude/skills/ 對 CLI 的相依。

Requirements:
  REQ-001:
    desc: 目錄結構符合 DDD 藍圖。重構後 src/ 下出現 4 個 bounded context + utils/，
          每個 context 內按需要含 dao.rs、service/、facade.rs。
    relevant_scenario: 1.2 — Context mod.rs 存在且註解職責（catalog/mod.rs 開頭有 //! doc）
  REQ-002:
    desc: Service 層與 DAO 層的依賴隔離。service 不 import rusqlite，也不 import 任何
          dao module；DAO 是唯一接觸 SQL 的層；facade 是唯一同呼 service + dao 的層。
    relevant_scenarios:
      - "2.1: grep -rn 'use rusqlite' src/*/service/ 無輸出"
      - "2.2: grep -rnE 'use crate::[a-z]+::dao' src/*/service/ 無輸出"
      - "2.4: facade.rs 可 import 自己 context 的 service 與 dao；不直接 import 其他 context 的 service / dao"
  REQ-003:
    desc: 對外介面 100% 不變（CLI grammar、SQLite schema、book-source JSON、config.toml key）。
    relevant_scenario: "3.2: SQLite schema 不變（sources / novels / chapters / progress 四張表）
                       — SCHEMA 字串留在 library/dao.rs::open()，不在 catalog/dao.rs 重複"

Scenarios:
  - id: REQ-001 Scen 1.2
    given: 4 個 context 目錄都建立
    when: Read src/catalog/mod.rs
    then: 檔案最上方有 //! doc comment 描述 Catalog 職責與對外 PL
  - id: REQ-002 Scen 2.1
    given: 重構完成
    when: grep -rn "use rusqlite" src/*/service/
    then: 無輸出（catalog/service/source.rs, rule.rs, scraper.rs 都不能 import rusqlite）
  - id: REQ-002 Scen 2.2
    given: 重構完成
    when: grep -rnE "use crate::[a-z]+::dao" src/*/service/
    then: 無輸出（catalog/service/* 不能 import catalog::dao 或 library::dao）
  - id: REQ-002 Scen 2.4
    given: 重構完成
    when: Read src/catalog/facade.rs
    then: 該檔案 import 自己 context 的 service 與 dao 模組（catalog::service::scraper、
          catalog::dao），允許借用 &LibraryDb（library 暴露的 type），但**不**直接
          import library::dao 的 method
  - id: REQ-003 Scen 3.2
    given: 既有 ~/.local/share/novel-looker/novel-looker.db
    when: 重構後 binary 開啟同 DB
    then: 不出現 "no such column"；schema 與重構前完全一致（SCHEMA DDL 仍由
          library/dao.rs::LibraryDb::open() 一次 execute_batch，catalog/dao.rs
          只搬 method 不搬 CREATE TABLE 字串）

Task:
  id: TASK-catalog-03
  title: catalog/dao.rs（sources 表 CRUD + 共享 connection）+ catalog/facade.rs
  goal: |
    把 sources 表相關方法（save_source / list_sources / get_source）從 library/dao.rs
    搬到 catalog/dao.rs（free function，第一參數借用 &LibraryDb 注入共用 SQLite
    connection）；replace_toc 同樣搬到 catalog/dao.rs（Shared Kernel — chapters
    TOC 屬 Catalog 寫）；建立 catalog/facade.rs 提供 search / fetch_novel_info /
    sync_toc / fetch_chapter_content 4 個 use case orchestrator。
  acceptance:
    - src/catalog/dao.rs 提供 save_source(&mut LibraryDb, &BookSource), list_sources(&LibraryDb),
      get_source(&LibraryDb, url), replace_toc(&mut LibraryDb, novel_id, &[ChapterMeta])
    - src/catalog/dao.rs 開頭 doc comment：
      "//! Catalog DAO. NOTE: Shared Kernel — sources 表 column + chapters.{idx,name,url}
      由 Catalog 寫；chapters.content 由 Library DAO 寫。所有方法借用 LibraryDb 的 Connection。"
    - src/catalog/facade.rs 提供 search / fetch_novel_info / sync_toc / fetch_chapter_content
      4 個函數，每個 doc-commented 對應 CLI subcommand
    - facade 是唯一同呼 service + dao 的層；service 仍不 import dao
    - cargo test + e2e（search / add / sync）通過
    - library/dao.rs 對應位置加 marker 註解（按 TRANSITION 規範；最終會被清零 gate 檢出）
  steps:
    catalog_dao:
      - 建立 src/catalog/dao.rs（含 //! Shared Kernel doc comment）
      - 從 src/library/dao.rs 剪下前 task 暫存的 save_source / list_sources / get_source 方法
        貼到 catalog dao；改為 free function 接 &LibraryDb / &mut LibraryDb 第一參數
      - 把 replace_toc 從 library dao 搬到 catalog dao（Shared Kernel — chapters TOC 寫）
      - library dao 對應位置加 "// MOVED: sources CRUD + chapters TOC writes 已搬到 catalog::dao" 註解
      - **不要搬 SCHEMA 字串**；CREATE TABLE 整體留在 library/dao::LibraryDb::open() 內
        一次 execute_batch。catalog/dao.rs 只搬 method
    catalog_facade:
      - 建立 src/catalog/facade.rs
      - 4 個 facade 函數簽名（按 Borrow 規則）：
          search(db: &LibraryDb, scraper: &Scraper, source_url: &str, keyword: &str)
            -> Result<Vec<SearchHit>>  // async（Cmd::Search）
          fetch_novel_info(scraper: &Scraper, source: &BookSource, book_url: &str)
            -> Result<Novel>  // async（Cmd::Add 用）
          sync_toc(db: &mut LibraryDb, scraper: &Scraper, source: &BookSource,
                   novel_id: i64, toc_url: &str) -> Result<usize>  // async；回傳章節數（Cmd::Sync）
          fetch_chapter_content(scraper: &Scraper, source: &BookSource, chapter_url: &str)
            -> Result<String>  // async（Cmd::Read cache miss）
      - facade 內部呼 Scraper::search/fetch_info/fetch_toc/fetch_content 取 raw data
        → 視需要呼 catalog::dao 寫 sources/TOC → 回 PL 型別
      - 每個 facade fn 加 doc comment：對應哪個 CLI subcommand
    cleanup_storage:
      - src/storage.rs 內 pub use ... 別名移除 sources / replace_toc 相關 method 重 export
        （cli.rs 已改呼 catalog::dao::* free function，不再透過 storage alias）
      - 若 storage.rs 完全空了（只剩 doc comment）刪除；src/main.rs 移除 mod storage;
    cli_update:
      - Cmd::Source::Import handler：store.save_source(s) → catalog::dao::save_source(&mut store, s)
      - Cmd::Source::List handler：store.list_sources() → catalog::dao::list_sources(&store)
      - Cmd::Search handler：scraper.search(src, keyword).await
        → catalog::facade::search(&store, &scraper, &src.book_source_url, &keyword).await
      - Cmd::Add handler：使用 catalog::facade::fetch_novel_info(&scraper, &src, &book_url)
        然後 library::facade::add_novel(&mut store, &novel)
      - Cmd::Sync handler：catalog::facade::sync_toc(&mut store, &scraper, &src, novel_id, toc_url)
      - Cmd::Read cache miss path：catalog::facade::fetch_chapter_content(&scraper, &src, chapter_url)
        + library::facade::save_chapter_content(&mut store, ...)
  verification:
    - cargo build（exit 0；新 warning ≤ 2 條基線）
    - cargo test（4 個 catalog::service::rule::tests 全綠）
    - cargo run -- source list（列出 2 個書源：gutenberg + uukanshu）
    - cargo run -- search alice（Gutenberg ≥ 5 筆 — 驗證 catalog::facade::search）
    - cargo run -- sync 3（印「✓ 同步 N 章」— 驗證 catalog::facade::sync_toc + Shared
      Kernel chapters write）
    - grep -rn "use rusqlite" src/catalog/service/  # 0 行
    - grep -rnE "use crate::[a-z]+::dao" src/catalog/service/  # 0 行

Design:
  catalog_dir_layout: |
    src/catalog/
    ├── mod.rs          // //! Catalog: 描述如何從網站抽資料並執行抽取（已建立）
    ├── facade.rs       // 本 task 建立：search / fetch_novel_info / sync_toc / fetch_chapter_content
    ├── dao.rs          // 本 task 建立：sources CRUD + replace_toc（Shared Kernel）
    └── service/
        ├── mod.rs
        ├── source.rs   // BookSource + sub-rule struct（前 task 已搬）
        ├── rule.rs     // rule DSL（前 task 已搬）
        └── scraper.rs  // Scraper（wreq + Chrome 131；前 task 已搬）

  shared_kernel: |
    sources 表 columns + chapters.{idx, name, url} 由 Catalog DAO 寫；
    chapters.content 由 Library DAO 寫。兩個 DAO 共用同一條 SQLite Connection
    （透過借用 &LibraryDb 注入）。修改任一方 schema 需同步檢視對方 DAO。

    在 catalog/dao.rs 與 library/dao.rs 開頭都加：
    //! NOTE: Shared Kernel — sources.* 與 chapters.{idx,name,url} 由 Catalog 寫；
    //! chapters.content 由 Library 寫。修改任一方 schema 需同步檢視對方 DAO。

  db_connection_sharing: |
    rusqlite 預設 single-threaded mode；不讓每個 context 各自 Connection::open
    原因：
    1. 多 Connection 對同 DB 在序列操作下會 SQLITE_BUSY
    2. backup 內部跨 DAO 操作需一致 snapshot
    3. sync + backup 連續執行不必設 busy_timeout（test.md E14）
    4. refactor 約束不能引入 WAL — 共享 connection 是最簡解

  borrow_rules:
    read_only:    "fn xxx(db: &LibraryDb, ...) -> Result<T>  // list_sources, get_source"
    single_write: "fn xxx(db: &mut LibraryDb, ...) -> Result<T>  // save_source"
    transaction:  "fn xxx(db: &mut LibraryDb, ...) -> Result<T>  // replace_toc"

  LibraryDb_interface: |
    pub struct LibraryDb { conn: Connection }
    impl LibraryDb {
        pub fn open() -> Result<Self>
        pub fn conn(&self) -> &Connection
        pub fn conn_mut(&mut self) -> &mut Connection
    }
    catalog::dao 函數透過 db.conn() / db.conn_mut() 取 rusqlite Connection 操作

  facade_topology: |
    handler 可呼三個 context 的 facade；facade 不互呼（除 backup→library Conformist 例外）。
    catalog::facade 不能呼 library::facade — Catalog 與 Library 是 Shared Kernel +
    分流 access，不是 Conformist 關係。
    handler 是唯一跨 context 編排點（add: catalog::facade + library::facade 兩段；
    read cache miss: catalog::facade + library::facade 兩段）。

  add_sync_read_data_flow: |
    add：
      H -> CF::fetch_novel_info -> CS scraper HTTP + ruleBookInfo -> Novel
      H -> LF::add_novel -> LD upsert_novel -> DB
    sync：
      H -> LF::get_novel -> Novel (含 source_url, toc_url)
      H -> CF::sync_toc(&mut db, scraper, source, novel_id, toc_url) 內部：
           CS::fetch_toc -> Vec<ChapterMeta> -> catalog::dao::replace_toc(&mut db, novel_id, &chapters)
           回傳 chapters.len()
    read cache miss：
      H -> LF::get_chapter -> ChapterMeta (no content)
      H -> CF::fetch_chapter_content -> RawContent String
      H -> LF::save_chapter_content -> LD UPDATE chapters SET content

  error_handling: |
    維持 anyhow。
    - catalog/dao：rusqlite::Error 用 anyhow::Context 加路徑提示再向上拋
    - catalog/service/scraper：HTTP / parse 失敗加上下文（哪個 URL / 哪條 rule）
    - catalog/facade：聚合多步驟錯誤，不另外包；保留 ? chain

  transition_marker_gate: |
    refactor 過程中所有暫時別名 / 過渡 re-export / 暫留檔案，強制標
    // TRANSITION: 註解。最終 PR 前 grep 必須為空：
      grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/
    本 task 在 library/dao.rs 加 "// MOVED: sources CRUD + chapters TOC writes 已搬到 catalog::dao"
    屬於暫時 marker；後續 task-presentation-04 自檢時會被清零。

Test:
  unit:
    - cargo test catalog::service::rule::tests  # 4 case（前 task 已通過，本 task 不破壞）

  e2e_relevant_to_this_task:
    E1:  cargo test 全綠 / cargo build 0 error
    E3:  sqlite3 .schema 與 baseline 比對一致（SCHEMA 字串未改）
    E7:  cargo run -- sync $NID 印「✓ 同步 N 章」（N 與 setup.md baseline 一致）
    E13: 再跑 sync 後 progress.chapter_index 數字不變（replace_toc 內 transaction 不洗 progress）
    E14: cargo run -- sync $NID && cargo run -- backup 不出「database is locked」
         （驗證共享 Connection 設計）

  integration_relevant:
    - "Handler → catalog/library facade：handler 只透過 facade 介面溝通"
    - "Facade → service + DAO：catalog/facade.rs 是唯一同時 import catalog::service::scraper
       與 catalog::dao 的檔案"
    - "Catalog DAO + Library DAO 共享 connection：兩個 DAO 透過同一個 Connection 操作；
       不出現 SQLITE_BUSY"
    - "Shared Kernel 寫入欄位分工：sync 後 idx/name/url 非空、content length=0；
       read 後 content length>0"

  boundary_conditions:
    - "REQ-002 Scen 2.1：grep -rn 'use rusqlite' src/catalog/service/ 無輸出"
    - "REQ-002 Scen 2.2：grep -rnE 'use crate::[a-z]+::dao' src/catalog/service/ 無輸出"
    - "facade 不互呼跨 context（除 backup→library 例外）：catalog/facade.rs 不 import
       crate::library::facade"
    - "DB connection 不重複開：catalog/dao.rs 內無 Connection::open 或 LibraryDb::open
       呼叫（透過 &LibraryDb 借用）"
    - "Cloudflare bypass 路徑保留：grep -rn 'Emulation::Chrome131' src/catalog/service/scraper.rs
       恰有 1 行（前 task 已驗證；本 task 不動 scraper.rs）"
    - "Cache miss 流程正確：read 未抓過章節時 fetch+cache+印出；第二次同章節不發 HTTP
       （E7b — 驗證 catalog::facade::fetch_chapter_content 只在 cache miss 由 handler 呼叫）"
    - "編譯 warning ≤ 基線 2 條（dead_code: select_within, extract_all_doc）"

Constraints:
  hard:
    - LIBCLANG_PATH=/usr/lib/llvm-18/lib 為 cargo build 必要 prefix（wreq -> boring -> bindgen）
    - 在 branch refactor/ddd-context-split 上作業
    - .claude/skills/ 一字不改（REQ-006 Scen 6.1：git diff --stat .claude/skills/ 須無輸出）
    - SQLite schema 完全不變（REQ-003 Scen 3.2）；CREATE TABLE 字串只留在
      library/dao.rs::LibraryDb::open() 一處，**不**在 catalog/dao.rs 重複
    - SCHEMA 字串不搬走；catalog/dao.rs 只搬 method
    - service 層不 import dao（REQ-002 Scen 2.2）；catalog/service/* 不能引用 catalog::dao
    - service 層不 import rusqlite（REQ-002 Scen 2.1）
    - facade 不互呼跨 context（catalog::facade 不能 use crate::library::facade）
    - DB connection 不重複開；catalog::dao 不能 Connection::open 或自呼 LibraryDb::open
      （透過 &LibraryDb 借用）
    - 不引入 thiserror / domain error type；維持 anyhow（KD 7 — 不過早抽象）
    - 不修改 CLI grammar / config.toml key / book-source JSON 欄位
    - cargo build 新 warning ≤ 2 條基線；無 unused_imports 殘留
    - replace_toc 內 transaction 不破壞 progress（E13）
    - Cloudflare bypass 路徑保留（wreq + Emulation::Chrome131）；本 task 不動 scraper.rs

  transition_marker:
    - library/dao.rs 移除位置加 "// MOVED: sources CRUD + chapters TOC writes 已搬到 catalog::dao"
      （屬本次 refactor 允許的 TRANSITION marker；最終 task-presentation-04 清零 gate 移除）

  shared_kernel_invariants:
    - sources 表 columns + chapters.{idx, name, url} 由 Catalog DAO 寫
    - chapters.content 由 Library DAO 寫（不變更）
    - 兩個 DAO 共用同一條 LibraryDb 的 Connection
    - 修改任一方 schema 需同步檢視對方 DAO

Files:
  create:
    - src/catalog/dao.rs       # //! Shared Kernel doc + save_source / list_sources / get_source / replace_toc free fn
    - src/catalog/facade.rs    # search / fetch_novel_info / sync_toc / fetch_chapter_content（各 async）

  modify:
    - src/library/dao.rs       # 移除 save_source / list_sources / get_source / replace_toc 4 method；
                               # 對應位置加 "// MOVED: ... 已搬到 catalog::dao" 註解
    - src/storage.rs           # 移除 sources / replace_toc 相關 pub use re-export；若整檔空則刪除
    - src/main.rs              # 若 storage.rs 被刪除，移除 mod storage;
    - src/cli.rs               # Cmd::Source::Import/List → catalog::dao::*
                               # Cmd::Search → catalog::facade::search
                               # Cmd::Add → catalog::facade::fetch_novel_info + library::facade::add_novel
                               # Cmd::Sync → catalog::facade::sync_toc
                               # Cmd::Read cache miss → catalog::facade::fetch_chapter_content
                               #   + library::facade::save_chapter_content

  do_not_touch:
    - src/catalog/service/source.rs   # 前 task 已定型
    - src/catalog/service/rule.rs     # 前 task 已定型
    - src/catalog/service/scraper.rs  # 前 task 已定型；Emulation::Chrome131 必須保留
    - src/catalog/mod.rs              # 前 task 已建立 doc + SearchHit PL
    - src/library/mod.rs              # library group 已定型
    - src/library/facade.rs           # library group 已定型
    - .claude/skills/**               # REQ-006 不可變更
    - 任何 SQLite CREATE TABLE 字串    # SCHEMA 留在 library/dao.rs::open()

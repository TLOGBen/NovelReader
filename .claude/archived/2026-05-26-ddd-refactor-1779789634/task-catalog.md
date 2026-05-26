# Tasks: catalog
**前置群組**：library

## TASK-catalog-01: 建立 catalog/ 結構 + 搬 BookSource 與 Rule DSL

**需求追溯**：REQ-001 (Scen 1.1, 1.2, 1.3), REQ-002 (Scen 2.1, 2.2), REQ-005
**目標**：`src/catalog/{mod.rs, service/{mod.rs, source.rs, rule.rs}}` 建立；`src/source/{mod.rs, rule.rs}` 內容搬入；既有 `cargo test source::rule::tests` 4 個案例改 path 後仍通過。

**驗收標準**：
- [ ] `src/catalog/mod.rs` 含 `//! Catalog: 描述如何從網站抽資料並執行抽取` doc + Outbound PL 列表（SearchHit / Novel / Vec<ChapterMeta> / RawContent）
- [ ] `catalog/service/source.rs` 含 `BookSource` 及所有 sub-rule struct（從 `src/source/mod.rs` 搬）
- [ ] `catalog/service/rule.rs` 含 rule DSL（從 `src/source/rule.rs` 搬，含 4 個 #[test]）
- [ ] `src/source/` 目錄刪除
- [ ] cargo test 4 個 rule tests 通過（新 path `catalog::service::rule::tests`）

### 步驟

#### 建立 catalog 模組
- [ ] `mkdir -p src/catalog/service`
- [ ] `src/catalog/mod.rs`：doc comment + `pub mod service; pub mod dao; pub mod facade;`（後兩個下 task 建立）
- [ ] `src/catalog/service/mod.rs`：`pub mod source; pub mod rule; pub mod scraper;`（scraper 下 task 搬）

#### 搬移程式碼
- [ ] 把 `src/source/mod.rs` 整個內容貼到 `src/catalog/service/source.rs`，移除 `pub mod rule;` 那行（rule 改到 service/mod.rs 註冊）
- [ ] 把 `src/source/rule.rs` 整個內容貼到 `src/catalog/service/rule.rs`，**包含** #[cfg(test)] mod tests 區塊
- [ ] `rm -rf src/source/`
- [ ] `src/main.rs` 移除 `mod source;`，加 `mod catalog;`

#### 更新 import
- [ ] `grep -rn "use crate::source::" src/`：把所有引用改為 `use crate::catalog::service::source::*` 或 `use crate::catalog::service::rule::*`
- [ ] 注意 `BookSource` 仍要被 storage.rs / cli.rs / backup.rs 看見：暫時 `pub use` 在 `src/catalog/mod.rs` 內 re-export：`pub use service::source::BookSource;`

#### 驗證
- [ ] `cargo build`
- [ ] `cargo test catalog::service::rule::tests`（4 個 case 通過）

---

## TASK-catalog-02: 搬 Scraper + SearchHit (Catalog PL)

**需求追溯**：REQ-001 (Scen 1.3), REQ-004 (Scen 4.2 — Cloudflare bypass 路徑保留), REQ-005
**目標**：`Scraper` 從 `src/scraper.rs` 搬到 `src/catalog/service/scraper.rs`；`SearchHit` 從 `src/models.rs` 搬到 `src/catalog/mod.rs`（PL re-export）；`src/models.rs` 與 `src/scraper.rs` 刪除。

**驗收標準**：
- [ ] `src/catalog/service/scraper.rs` 含完整 Scraper（HTTP + 套規則的 `search/fetch_info/fetch_toc/fetch_content`）
- [ ] `Scraper::new()` 仍用 `wreq::Client::builder().emulation(Emulation::Chrome131)`（Cloudflare bypass 不被破壞）
- [ ] `SearchHit` 在 `src/catalog/mod.rs` 內定義或 `pub use`，明確標為 PL（doc comment）
- [ ] `src/models.rs` 內容空（全搬走 — Novel 等已在 library group 搬走，SearchHit 此 task 搬走），整個檔案刪除
- [ ] `src/scraper.rs` 刪除
- [ ] cargo build + test 通過

### 步驟

#### 搬 Scraper
- [ ] 把 `src/scraper.rs` 內容（去掉已搬走的 `resolve`、`normalize_paragraphs`）貼到 `src/catalog/service/scraper.rs`
- [ ] `normalize_paragraphs` 留在 `scraper.rs`（私有 helper；非跨 module 共用，所以不搬到 utils）
- [ ] `src/main.rs` 移除 `mod scraper;`

#### 搬 SearchHit
- [ ] 從 `src/models.rs` 剪 `SearchHit` 定義
- [ ] 貼到 `src/catalog/mod.rs`，加 doc comment：`/// PL: Catalog 對外發佈的搜尋結果型別。Published Language across context boundary.`
- [ ] `rm src/models.rs`，`src/main.rs` 移除 `mod models;`

#### 更新引用
- [ ] `grep -rn "use crate::scraper::" src/`：改為 `use crate::catalog::service::scraper::Scraper`
- [ ] `grep -rn "use crate::models::SearchHit" src/`：改為 `use crate::catalog::SearchHit`

#### 驗證
- [ ] `cargo build`
- [ ] `cargo run -- search "alice"`（Gutenberg）回傳 ≥ 5 筆
- [ ] `cargo run -- add --source https://uukanshu.cc https://uukanshu.cc/book/21940/`：印「✓ 加入書架 (#N) 超維術士 / 佚名」（驗證 wreq + Chrome 131 仍生效）

---

## TASK-catalog-03: catalog/dao.rs（sources 表 CRUD + 共享 connection）+ catalog/facade.rs

**需求追溯**：REQ-001 (Scen 1.2), REQ-002 (Scen 2.1, 2.2, 2.4), REQ-003 (Scen 3.2)
**目標**：把 `src/storage.rs` 內 sources 表相關方法（`save_source / list_sources / get_source`）搬到 `src/catalog/dao.rs`，**共用 Library 的 SQLite connection**（透過 `&LibraryDb` 參數注入）；`src/catalog/facade.rs` 提供 search / fetch_novel_info / sync_toc / fetch_chapter_content 對外 entry point。

**驗收標準**：
- [ ] `src/catalog/dao.rs` 提供 `save_source(&LibraryDb, &BookSource)`, `list_sources(&LibraryDb)`, `get_source(&LibraryDb, url)`, `replace_toc(&LibraryDb, novel_id, &[ChapterMeta])`（後者是 Shared Kernel TOC 寫入）
- [ ] `src/catalog/dao.rs` 開頭 doc comment：`//! Catalog DAO. NOTE: Shared Kernel — sources 表 column + chapters.{idx,name,url} 由 Catalog 寫；chapters.content 由 Library DAO 寫。所有方法借用 LibraryDb 的 Connection。`
- [ ] `src/catalog/facade.rs` 提供：`search(&LibraryDb, scraper, source_url, keyword)`, `fetch_novel_info(scraper, source, book_url)`, `sync_toc(&LibraryDb, scraper, source, novel_id, toc_url)`, `fetch_chapter_content(scraper, source, chapter_url)`
- [ ] facade 是唯一同呼 service + dao 的層；service 仍不 import dao
- [ ] cargo test + e2e（search / add / sync）通過

### 步驟

#### catalog/dao.rs
- [ ] 建立 `src/catalog/dao.rs`
- [ ] 從 `src/library/dao.rs`（前 group 暫存的 sources 方法）剪下 `save_source / list_sources / get_source` 方法，貼到 catalog dao；改為 free function 接 `&LibraryDb` 第一參數
- [ ] 把 `replace_toc` 從 library dao 搬到 catalog dao（**Shared Kernel — chapters TOC 屬 Catalog 寫**）
- [ ] library dao 對應位置加 `// TRANSITION: chapters TOC writes 已搬到 catalog::dao`（marker 統一見 design.md `_legacy` 清零 gate 段）
- [ ] **不要搬 SCHEMA 字串**；schema DDL 整體留在 `library/dao::LibraryDb::open()` 內一次 `execute_batch`。catalog/dao.rs 只搬 method 不搬 `CREATE TABLE` 字串

#### catalog/facade.rs
- [ ] 建立 `src/catalog/facade.rs`
- [ ] 寫 4 個 facade 函數，每個都註解對應 CLI subcommand
- [ ] facade 內部呼 `Scraper::search(...)` 等取 raw data → 視需要呼 dao 寫 sources/TOC → 回 caller 的 PL 型別

#### 清理 storage.rs
- [ ] `src/storage.rs` 內 `pub use ...` 別名移除 sources 相關方法（避免兩處被 import）
- [ ] 若 storage.rs 完全空了（只剩 doc comment），刪除檔案；`src/main.rs` 移除 `mod storage;`

#### 更新 cli.rs 暫時引用（presentation group 才會徹底拆 cli.rs）
- [ ] cli.rs 內呼 `store.save_source(...)` 改為 `catalog::dao::save_source(&store, ...)`，類推其他 sources / TOC 呼叫

#### 驗證
- [ ] `cargo build`
- [ ] `cargo test`
- [ ] `cargo run -- source list`：列出 2 個書源（gutenberg + uukanshu）
- [ ] `cargo run -- search alice`：Gutenberg 仍可搜
- [ ] `cargo run -- sync $NID`：印「✓ 同步 N 章」（N 為實際章節數，與 setup.md baseline 一致）
- [ ] `grep -rn "use rusqlite" src/catalog/service/`：無輸出
- [ ] `grep -rnE "use crate::[a-z]+::dao" src/catalog/service/`：無輸出

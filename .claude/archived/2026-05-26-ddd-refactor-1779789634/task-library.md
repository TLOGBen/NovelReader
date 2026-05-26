# Tasks: library
**前置群組**：shared

## TASK-library-01: 建立 library/ 結構 + 搬移 Library 專屬 type

**需求追溯**：REQ-001 (Scen 1.1, 1.2), REQ-002 (Scen 2.1, 2.3), REQ-005
**目標**：`src/library/{mod.rs, dao.rs, facade.rs, service/mod.rs}` 骨架完成；`Novel`、`ChapterMeta`、`Chapter`、`ReadProgress` 四個 type 從 `models.rs` 搬入 `library/mod.rs`（pub re-export）。

**驗收標準**：
- [ ] `src/library/{mod.rs, dao.rs, facade.rs}` 與 `src/library/service/mod.rs` 存在
- [ ] `library/mod.rs` 開頭有 `//! Library: 維護書架 / TOC / 章節快取 / 進度` doc comment + Outbound PL 列表
- [ ] `Novel / ChapterMeta / Chapter / ReadProgress` 在 `library/mod.rs` 內定義（或 sub-module 內 + pub re-export）
- [ ] `src/models.rs` 刪除以上 4 個 type（保留 `SearchHit` — 屬 Catalog，下個 group 處理）
- [ ] 全 codebase 對這 4 個 type 的 import 更新為 `crate::library::*`
- [ ] cargo build + test 通過

### 步驟

#### 建立目錄結構
- [ ] `mkdir -p src/library/service`
- [ ] 建立 `src/library/mod.rs`：doc comment + `pub mod dao; pub mod facade; pub mod service;` + 待搬入的 type
- [ ] 建立 `src/library/dao.rs`：佔位 `// TODO: SQL access for novels / chapters.content / progress`
- [ ] 建立 `src/library/facade.rs`：佔位
- [ ] 建立 `src/library/service/mod.rs`：佔位 + doc comment

#### 搬移 type
- [ ] 從 `src/models.rs` 剪下 `Novel`、`ChapterMeta`、`Chapter`、`ReadProgress` 定義
- [ ] 貼入 `src/library/mod.rs`（保留 `Serialize / Deserialize` derive）
- [ ] `src/main.rs` 加 `mod library;`

#### 更新 import
- [ ] `grep -rn "use crate::models::" src/` 找出所有引用
- [ ] 對 `Novel / ChapterMeta / Chapter / ReadProgress` 的引用改為 `use crate::library::{Novel, ...};`
- [ ] `SearchHit` 暫留 `crate::models::SearchHit`（下個 group 處理）

#### 驗證
- [ ] `cargo build --bin novel-looker`，warning 不增加
- [ ] `cargo test`

---

## TASK-library-02: 搬移 DAO 方法 + `open_db()` factory

**需求追溯**：REQ-001 (Scen 1.3), REQ-002 (Scen 2.3 — DAO 是唯一接 SQL 的層), REQ-003 (Scen 3.2 — schema 不變), REQ-004 (Scen 4.1)
**目標**：把 `src/storage.rs` 內**屬 Library 的 SQL 方法**（novels / chapters.content / progress 相關）搬到 `src/library/dao.rs`；保留 `open_db()` factory 在 `library/dao.rs` 作為共享 SQLite connection 入口。Catalog / Backup DAO 之後將注入此 connection。

**驗收標準**：
- [ ] `src/library/dao.rs` 提供：
  - `pub fn open_db() -> Result<Connection>`（或 `pub struct LibraryDb { conn: Connection }`，配套 `pub fn open() -> Result<Self>`；二選一，建議後者以利 RAII）
  - `upsert_novel`, `list_novels`, `get_novel`, `replace_toc`, `list_chapters`, `get_chapter`, `save_chapter_content`, `save_progress`, `get_progress`
- [ ] 上述方法簽名與舊 `Storage::*` 等價（return type / 參數 / 錯誤型別一致）
- [ ] **schema 字串保留**：`SCHEMA` const 包含 sources / novels / chapters / progress 四表（不可只保留 Library 三表，否則 Catalog 還沒 refactor 完會炸；Catalog 的 sources table 操作下 group 才搬）
- [ ] `src/storage.rs` 暫保留為 `pub use crate::library::dao::*;` 別名（讓現有 cli.rs / backup.rs 不爆；下個 group 內逐步切換 import）
- [ ] cargo build + test 通過
- [ ] **E3 schema diff**：`sqlite3 .schema` 對比重構前一致

### 步驟

#### 重寫 library/dao.rs
- [ ] 從 `src/storage.rs` 複製 `SCHEMA` const 與 `pub struct Storage { conn: Connection }`，改名 `LibraryDb`
- [ ] 複製 `Storage::open()` 改名 `LibraryDb::open()`，路徑與錯誤訊息保留
- [ ] 複製 Library 相關方法：`upsert_novel`, `list_novels`, `get_novel`, `replace_toc`, `list_chapters`, `get_chapter`, `save_chapter_content`, `save_progress`, `get_progress`
- [ ] 在檔案頂部加 doc comment：`//! Library DAO. NOTE: Shared Kernel — chapters.{idx,name,url} 由 Catalog DAO 寫；本檔僅負責 chapters.content / novels / progress。sources 表 schema 含於 SCHEMA 但 CRUD 在 catalog/dao.rs。`

#### 保留 storage.rs 別名（含 TRANSITION marker）
- [ ] `src/storage.rs` 內容改為：
  ```rust
  // TRANSITION: removed in task-presentation-02 cleanup
  //! 過渡別名：所有 Library DAO 重新導出。catalog/backup refactor 完成後刪除此檔。
  pub use crate::library::dao::LibraryDb as Storage;
  pub use crate::library::dao::*;
  ```
- [ ] 確認既有 `use crate::storage::Storage` 仍可用（cli.rs / backup.rs 的呼叫不破）
- [ ] **借用簽名**：`LibraryDb` 提供 `pub fn open() -> Result<Self>` + `pub fn conn_mut(&mut self) -> &mut Connection`；DAO method 按 design.md「Borrow 規則」表分 `&self` (read) vs `&mut self` (write/transaction)。`replace_toc` 為 `&mut self`

#### 驗證
- [ ] `cargo build` + `cargo test`
- [ ] 用既有 DB（setup.md baseline 中的 N 本書）跑 `cargo run -- shelf`，輸出書數與 baseline 一致
- [ ] 跑 `cargo run -- read $NID 0`，內文顯示第 1 章（行數 > 5）

---

## TASK-library-03: facade + service 骨架（薄包裝）

**需求追溯**：REQ-001 (Scen 1.2), REQ-002 (Scen 2.4 — facade 可同呼 service+dao)
**目標**：`library/facade.rs` 提供 use case 入口（thin wrapper over DAO，等下個 step 讓 Catalog/Backup/Presentation 呼）；`library/service/shelf.rs` 建立但**內容暫空**（只有 `mod` 宣告 + doc comment），不擅自抽出未經要求的業務邏輯。

**驗收標準**：
- [ ] `library/facade.rs` 提供至少 thin wrapper：`add_novel(db, novel)`, `list_shelf(db)`, `get_novel(db, id)`, `get_chapter(db, id, idx)`, `save_chapter_content(db, id, idx, content)`, `replace_toc(db, id, chapters)`, `save_progress(db, progress)`, `get_progress(db, id)`
- [ ] facade 函數**只**呼 DAO，不直接寫 SQL
- [ ] `library/service/shelf.rs` 存在，僅含 doc comment 與 `// TODO: invariants — TOC sync 不破壞 progress`
- [ ] cargo build + test 通過

### 步驟

#### 寫 library/facade.rs
- [ ] 對每個 DAO 方法寫一個對應 facade 函數，接收 `&LibraryDb` 為第一參數，內部直接 delegate
- [ ] 不引入新邏輯（純 pass-through；invariant enforcement 留 OQ）
- [ ] 加 doc comment：每個 facade fn 一句說明 use case 對應到哪個 CLI subcommand

#### 建立 service 骨架
- [ ] `src/library/service/shelf.rs` 內容：
  ```rust
  //! Library service: pure domain logic.
  //! NOTE: 目前無 invariant 邏輯需要從別處抽出；待 annotation / multi-session
  //! 等功能落地時，TOC↔progress 一致性檢查可在此實作。
  ```
- [ ] `library/service/mod.rs` 加 `pub mod shelf;`

#### 驗證
- [ ] `cargo build`
- [ ] `grep -rn "use rusqlite" src/library/service/`：無輸出（service 沒碰 SQL）
- [ ] `grep -rnE "use crate::library::dao" src/library/service/`：無輸出

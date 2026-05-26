Goal: |
  按 DDD 藍圖，把 novel-looker 從扁平結構重構為 4 個 bounded context（Catalog / Library / Backup / Presentation）× 5 層架構（action / facade / service / DAO / utils）的目錄結構，且不破壞任何使用者可見的對外介面（CLI grammar、SQLite schema、book-source JSON 格式、config.toml key、.claude/skills/ 相依）。
  本 task 為 Library context 的第一步：建立 library/ 骨架目錄，並把 Library 專屬的四個型別（Novel / ChapterMeta / Chapter / ReadProgress）從 src/models.rs 搬入 src/library/mod.rs。SearchHit 屬 Catalog，下一個 group 處理，本 task 不動。

Requirements:
  - REQ-001: |
      目錄結構符合 DDD 藍圖。重構後 src/ 下出現 4 個 bounded context 目錄（catalog/ library/ backup/ presentation/）+ utils/，每個 context 內按需要含 dao.rs、service/、facade.rs。
  - REQ-002: |
      Service 層與 DAO 層的依賴隔離。每個 context 內 service 層的 .rs 檔不直接 import SQL 相關 crate（rusqlite）也不 import 任何 dao module；DAO 是唯一接觸 SQL 的層；facade 是唯一同時呼叫 service 與 DAO 的層。
  - REQ-005: |
      編譯品質不退化。重構後編譯不出現新 warning；既有 warning（如 select_within dead_code）數量不增加。

Scenarios:
  REQ-001:
    - "Scenario 1.1 目錄結構檢查: Given 重構完成的 codebase, When `find src -type d -mindepth 1 -maxdepth 1 | sort`, Then 輸出包含 src/catalog, src/library, src/backup, src/presentation, src/utils 五個目錄, And 不包含 src/source。"
    - "Scenario 1.2 Context mod.rs 存在且註解職責: Given 4 個 context 目錄都建立, When Read src/catalog/mod.rs / src/library/mod.rs / src/backup/mod.rs / src/presentation/mod.rs, Then 每個檔案最上方有 //! doc comment 描述該 context 的 Purpose 與對外 PL。"
  REQ-002:
    - "Scenario 2.1 service 不 import rusqlite: When `grep -rn 'use rusqlite' src/*/service/ 2>/dev/null`, Then 無輸出。"
    - "Scenario 2.3 DAO 是唯一 import rusqlite 的層: When `grep -rln 'use rusqlite' src/ | grep -vE '(dao|storage)\\.rs'`, Then 無輸出。"
  REQ-005:
    - "Scenario 5.1 編譯 warning 不增加: 基線 cargo build warning = 2（select_within + BackupReceipt.filename）；重構後 warning 數量 ≤ 2，無 unused_imports。"
    - "Scenario 5.2 無 cargo check error: 每個 step 後 cargo check 退出碼 0。"

Task:
  id: TASK-library-01
  name: 建立 library/ 結構 + 搬移 Library 專屬 type
  目標: |
    src/library/{mod.rs, dao.rs, facade.rs, service/mod.rs} 骨架完成；Novel、ChapterMeta、Chapter、ReadProgress 四個 type 從 src/models.rs 搬入 src/library/mod.rs（pub re-export 或直接定義）。
  驗收標準:
    - "src/library/{mod.rs, dao.rs, facade.rs} 與 src/library/service/mod.rs 存在"
    - "library/mod.rs 開頭有 `//! Library: 維護書架 / TOC / 章節快取 / 進度` doc comment + Outbound PL 列表"
    - "Novel / ChapterMeta / Chapter / ReadProgress 在 library/mod.rs 內定義（或 sub-module + pub re-export）"
    - "src/models.rs 刪除以上 4 個 type（保留 SearchHit — 屬 Catalog，下個 group 處理）"
    - "全 codebase 對這 4 個 type 的 import 更新為 crate::library::*"
    - "cargo build + test 通過"
  步驟:
    建立目錄結構:
      - "mkdir -p src/library/service"
      - "建立 src/library/mod.rs：doc comment + `pub mod dao; pub mod facade; pub mod service;` + 待搬入的 type"
      - "建立 src/library/dao.rs：佔位 `// TODO: SQL access for novels / chapters.content / progress`"
      - "建立 src/library/facade.rs：佔位"
      - "建立 src/library/service/mod.rs：佔位 + doc comment"
    搬移 type:
      - "從 src/models.rs 剪下 Novel、ChapterMeta、Chapter、ReadProgress 定義"
      - "貼入 src/library/mod.rs（保留 Serialize / Deserialize derive）"
      - "src/main.rs 加 `mod library;`"
    更新 import:
      - "`grep -rn 'use crate::models::' src/` 找出所有引用"
      - "對 Novel / ChapterMeta / Chapter / ReadProgress 的引用改為 `use crate::library::{Novel, ...};`"
      - "SearchHit 暫留 crate::models::SearchHit（下個 group 處理）"
    驗證:
      - "LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker，warning 不增加（基線 2）"
      - "cargo test"

Design:
  library_context_目錄結構: |
    src/library/
    ├── mod.rs               # //! Library: 維護使用者書架 / TOC / 章節快取 / 進度
    ├── facade.rs            # add_novel() / list_shelf() / get_chapter() / save_progress()
    ├── dao.rs               # novels / chapters.content / progress 表 CRUD
    └── service/
        ├── mod.rs
        └── shelf.rs         # invariants: TOC sync 不破壞 progress (本 task 不建立 shelf.rs，留 task-library-03)

  型別放置策略:
    Novel:
      from: src/models.rs
      to: src/library/mod.rs（pub re-export）
      原因: Library 為 owner；Catalog 共用（Shared Kernel data type；本次不拆 ShelfEntry，留 OQ-6）
    ChapterMeta:
      from: src/models.rs
      to: src/library/mod.rs（pub re-export）
      原因: 同上
    Chapter:
      from: src/models.rs
      to: src/library/mod.rs（pub re-export）
      原因: 同上
    ReadProgress:
      from: src/models.rs
      to: src/library/mod.rs
      原因: Library 專屬
    SearchHit:
      from: src/models.rs
      to: src/catalog/mod.rs（pub re-export 為 PL）— 下一個 group 處理，本 task 不動
      原因: Catalog 對外 PL

  context_mod_rs_doc_comment_規範: |
    每個 context mod.rs 最上方必須有 //! doc comment 描述 Purpose 與對外 PL（從 .claude/wip/ddd-analysis.md §3 Canvas 萃取）。
    library/mod.rs 範例首行：`//! Library: 維護書架 / TOC / 章節快取 / 進度`
    並列出 Outbound PL（對外發布的 type list：Novel, ChapterMeta, Chapter, ReadProgress 等）。

  shared_kernel_註解: |
    chapters 表為 Shared Kernel：chapters.{idx,name,url} 由 Catalog DAO 寫，chapters.content 由 Library DAO 寫。本 task 尚未動 dao.rs 實作（佔位），但日後填內容時兩側 dao.rs 開頭都需加 Shared Kernel 註解。

  本_task_不做的事:
    - "不搬 SQL 方法到 dao.rs（留 TASK-library-02）"
    - "不寫 facade 函數（留 TASK-library-03）"
    - "不建 library/service/shelf.rs（留 TASK-library-03）"
    - "不動 SearchHit（留 catalog group）"
    - "不動 src/storage.rs（留 TASK-library-02 才改為別名）"

Test:
  本_task_驗證項目:
    - "E1 編譯 + 單元測試（每 step 後跑）：LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker + cargo test 全綠"
    - "E3 SQLite schema diff（每 step 後跑）：schema 不變（本 task 不動 dao，預期自然通過）"
    - "REQ-001 Scen 1.1 grep 檢查：`find src -type d -mindepth 1 -maxdepth 1 | sort` 應出現 src/library"
    - "REQ-001 Scen 1.2：library/mod.rs 開頭有 //! doc comment"
    - "REQ-005 Scen 5.1：cargo build warning 數量 ≤ 2（baseline = 2）；無 unused_imports"
    - "REQ-005 Scen 5.2：cargo check 退出碼 0"
  整合測試自檢_grep_類:
    - "搬移後 `grep -rn 'use crate::models::\\(Novel\\|ChapterMeta\\|Chapter\\|ReadProgress\\)' src/` 應無輸出"
    - "`grep -rn 'use crate::models::SearchHit' src/` 仍可有輸出（本 task 不動）"
  邊界條件:
    - "Mod 路徑變動不留 dead use：cargo build 不出現 unused_imports"
    - "models.rs 留下 SearchHit 後仍應編譯通過（不能整個刪掉檔案）"

Constraints:
  build_command: "LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker（LIBCLANG_PATH 為 BoringSSL via wreq 必需）"
  baseline_warnings: 2
  baseline_warnings_明細: "select_within (dead_code) + BackupReceipt.filename — 重構後 ≤ 2，不可新增"
  branch: "refactor/ddd-context-split"
  禁忌:
    - ".claude/skills/ 一個字元都不能改（REQ-006 Scen 6.1：`git diff --stat .claude/skills/` 必須無輸出）"
    - "不可改動 SQLite schema（C4：sources / novels / chapters / progress 四張表 schema 不變）"
    - "不可改動 CLI grammar / config.toml key / book-source JSON 欄位（C3, C6）"
    - "不可動 src/source/, src/scraper.rs, src/backup.rs, src/reader.rs, src/cli.rs（不屬本 task 範圍）"
    - "不可動 src/models.rs::SearchHit（屬 Catalog，下個 group 處理）"
    - "不可動 src/storage.rs（屬 TASK-library-02）"
    - "不可寫 SQL / DAO 實作（dao.rs / facade.rs 本 task 只是佔位）"
    - "不可寫 service/shelf.rs（屬 TASK-library-03）"
    - "保留 Serialize / Deserialize derive（搬 type 時不可遺漏）"
  transition_marker_convention: |
    本 task 若需暫時別名，用 `// TRANSITION: removed in task-<group>-<id> cleanup` 標記。
    範例：`// TRANSITION: removed in task-presentation-02 cleanup`
    最終 PR 前 `grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/` 必須無輸出（但本 task 屬 refactor 中段，TRANSITION 仍可暫存）。
  coordination_前後依賴:
    - "上游 TASK-shared-01 已完成：utils/ 目錄存在、resolve() 已搬移"
    - "下游 TASK-library-02 將：填 library/dao.rs（搬 SQL 方法 + open_db() factory），並把 src/storage.rs 改為 `pub use crate::library::dao::*;` 別名（含 TRANSITION marker）"
    - "下游 TASK-library-03 將：填 library/facade.rs（thin wrapper over DAO） + 建立 library/service/shelf.rs（doc comment 骨架）"

Files:
  將新增:
    - "src/library/mod.rs（doc comment + `pub mod dao; pub mod facade; pub mod service;` + Novel / ChapterMeta / Chapter / ReadProgress 定義）"
    - "src/library/dao.rs（佔位，含 TODO comment）"
    - "src/library/facade.rs（佔位）"
    - "src/library/service/mod.rs（佔位 + doc comment）"
  將修改:
    - "src/models.rs（刪除 Novel / ChapterMeta / Chapter / ReadProgress，保留 SearchHit）"
    - "src/main.rs（加 `mod library;` 宣告）"
    - "其他 src/**/*.rs 中所有 `use crate::models::{Novel|ChapterMeta|Chapter|ReadProgress}` 引用更新為 `use crate::library::{...}`（透過 grep 搜尋逐一改）"
  不可修改:
    - ".claude/skills/**/*"
    - "src/storage.rs"
    - "src/cli.rs（後續 group 處理）"
    - "src/scraper.rs / src/source/* / src/backup.rs / src/reader.rs / src/config.rs"
    - "book-sources/*.json / examples/*.json"
    - "SQLite schema（不改動 dao.rs 實作）"
  參考_不修改:
    - "/home/vakarve/projects/rust/novel-looker/.claude/analyze/2026-05-26-ddd-refactor/design.md（資料模型表 + library 結構）"
    - "/home/vakarve/projects/rust/novel-looker/.claude/wip/ddd-analysis.md §3 Canvas（library Outbound PL 來源）"

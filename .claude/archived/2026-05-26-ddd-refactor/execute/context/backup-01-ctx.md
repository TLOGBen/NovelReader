# Context: TASK-backup-01

```yaml
Goal: |
  Establish src/backup/ directory (4-layer DDD, NO dao.rs) and move
  Snapshot-related types + read/write functions (build_backup, export_to,
  import_from) into src/backup/service/snapshot.rs.

  Backup is a Conformist of Library — all storage access flows through
  crate::library::facade::*; Backup never imports rusqlite, never owns its
  own DAO. Other 3 contexts (Catalog / Library / Presentation) remain 5-layer.

  Per goal.md C8: src/backup/ must exist with mod.rs + facade.rs + service/.
  Per goal.md C1/C2: cargo build + cargo test stay green after this task.

Requirements:
  REQ-001: 目錄結構符合 DDD 藍圖
    重構後 src/ 出現 4 個 bounded context 目錄 + utils/，每個 context
    按需要含 dao.rs、service/、facade.rs。Backup 為 Conformist，刻意
    無 dao.rs（4 層）。

  REQ-002: Service 層與 DAO 層的依賴隔離
    service 不 import rusqlite；service 不 import 任何 dao module。
    對 Backup 而言更嚴格：service/snapshot.rs 全部 storage 操作必須
    走 crate::library::facade::*（Conformist 表達）。

Scenarios:
  Scen 1.1 (目錄結構檢查):
    Given 重構完成的 codebase
    When  find src -type d -mindepth 1 -maxdepth 1 | sort
    Then  輸出包含 src/backup（已存在 src/utils, src/catalog, src/library）

  Scen 1.2 (Context mod.rs 存在且註解職責):
    Given 4 個 context 目錄都建立
    When  Read src/backup/mod.rs
    Then  最上方 //! doc comment 描述 Backup Purpose 與對外 PL，
          並明確聲明「Conformist of Library — 無自有 DAO，
          storage access via library::facade」

  Scen 1.3 (舊扁平檔案已搬移 — 本 task 部份完成):
    本 task 後 src/backup.rs 仍存在（backup-02 才刪）；backup-01
    建立目錄結構並搬 Snapshot 區段。完整搬完留給 backup-02。

  Scen 2.1 (service 不 import rusqlite):
    grep -rn "use rusqlite" src/backup/service/ → 無輸出

  Scen 2.2 (service 不 import 任何 dao module):
    grep -rnE "use crate::[a-z]+::dao" src/backup/service/ → 無輸出
    （包含禁止 import library::dao；只能 library::facade）

Task: |
  TASK-backup-01: 建立 backup/ 結構 + 搬 Snapshot type 與 ACL mapping
  前置群組：library（已完成），catalog（已完成 — src/catalog/ 與
  src/library/ 結構已就位，library/facade 已提供需要的 wrappers）

  目標：
    src/backup/{mod.rs, facade.rs, service/{mod.rs, snapshot.rs, transport.rs}}
    建立（4 層，無 dao.rs）。Backup / BackedUpNovel / ProgressDump /
    ImportSummary 等 type + build_backup / export_to / import_from 搬到
    service/snapshot.rs。

  驗收標準：
    - 目錄結構完整（不含 dao.rs）
    - backup/mod.rs 開頭 doc comment：
      //! Backup: Library 狀態跨機器移動。Conformist of Library —
      //! 順從 Library DAO 既有 mutation API。
      （並標 Backup is 4-layer: no dao. Storage access via library::facade）
    - service/snapshot.rs 含 Snapshot 相關 type +
      build_backup(&LibraryDb) -> Result<Backup> +
      export_to(&LibraryDb, &Path) -> Result<usize> +
      import_from(&mut LibraryDb, &Path) -> Result<ImportSummary>
      （注意：import_from 需 &mut 因 upsert_novel/save_progress 是 &mut）
    - cargo build + test 通過
    - 舊資料 backup 與 export 功能性等價

  步驟（按 task-backup.md 原文）：
    建立 backup 模組
      - mkdir -p src/backup/service
      - src/backup/mod.rs：doc + pub mod facade; pub mod service;（無 dao）
      - src/backup/service/mod.rs：pub mod snapshot; pub mod transport;
      - src/main.rs：mod backup; （已存在）改為指向 backup/ dir

    搬 Snapshot type
      - 從 src/backup.rs 剪下 Backup / BackedUpNovel / ProgressDump /
        ImportSummary，貼到 src/backup/service/snapshot.rs
      - 同時搬 VERSION const 與 build_backup / export_to / import_from
      - 改為接 &LibraryDb（或 &mut）而非 &Storage
      - storage call 改 crate::library::facade::*

    驗證
      - cargo build
      - cargo run -- export /tmp/test-export.json 印「✓ 匯出 N 本書」

Design: |
  系統架構（design.md 摘錄）：
    src/backup/
    ├── mod.rs               # //! Backup: Library 狀態跨機器移動。
    │                        #   Conformist of Library — 透過 library::facade
    │                        #   達成 storage access，無自有 DAO 層。
    ├── facade.rs            # run_backup() / export_to() / import_from()
    │                        # （backup-01 只放 placeholder；backup-02 填入 run_backup）
    └── service/
        ├── mod.rs           # pub mod snapshot; pub mod transport;
        ├── snapshot.rs      # build_backup（read）/ apply_backup（write）+ ACL
        └── transport.rs     # push_local / push_webdav / prune_local（backup-02 填）

  Backup 為何 4 層（design.md L61）：
    Conformist of Library — Backup 的所有 storage 操作走 library::facade::*，
    沒有自己對 SQL 的直接接觸面，因此**不需要也不該有** backup/dao.rs。
    強行保留會是「為形式而抽象」的空殼。

  Borrow 規則（design.md L181-201）：
    SELECT (唯讀)     → fn xxx(db: &LibraryDb, ...)
    單一寫入          → fn xxx(db: &mut LibraryDb, ...)
    Transaction      → fn xxx(db: &mut LibraryDb, ...)
    → build_backup 接 &LibraryDb；import_from 接 &mut LibraryDb。

  facade 互呼例外（design.md L278-290）：
    backup/facade.rs **可以** import crate::library::facade::*
    （Conformist 合法）；其他 facade 不可互呼。

  Library facade 既有 wrapper（已就位於 src/library/facade.rs）：
    pub fn add_novel(&mut LibraryDb, &Novel) -> Result<i64>
    pub fn list_shelf(&LibraryDb) -> Result<Vec<Novel>>
    pub fn save_progress(&mut LibraryDb, &ReadProgress) -> Result<()>
    pub fn get_progress(&LibraryDb, i64) -> Result<Option<ReadProgress>>
    pub fn get_chapter / list_chapters / get_novel / save_chapter_content
    （NO list_novels 名稱 — 用 list_shelf；NO upsert_novel 名稱 — 用 add_novel）

  Type re-exports（design.md L161-163）：
    Novel / ReadProgress 已從 src/library/mod.rs 公開（pub struct）；
    snapshot.rs 用 use crate::library::{Novel, ReadProgress};

Test: |
  E2E（test.md 摘錄，本 task 相關 subset）：
    E1  cargo test 全綠（必跑）
    E3  SQLite schema diff vs baseline — schema 不能變
    E5  config.toml round-trip — config show 顯示 backup.backend/keep/local.path
    E8  backup → Drive — cargo run -- backup 印「✓ 備份 N 本書 [local] → ...」
        （E8 完整路徑要等 backup-02；backup-01 只跑 export 子集）

  關鍵邊界條件（本 task 必須過）：
    - grep -rn "use rusqlite" src/backup/service/ → 無輸出
    - grep -rnE "use crate::[a-z]+::dao" src/backup/service/ → 無輸出
      （即 src/backup/service/snapshot.rs 禁 import library::dao；
       只允許 library::facade 與 library::{Novel, ReadProgress}）
    - cargo build warning 數量 ≤ baseline（2 條 dead_code）
    - cargo run -- export /tmp/test.json 印「✓ 匯出 N 本書」並產出檔

Constraints:
  - LIBCLANG_PATH=/usr/lib/llvm-18/lib （所有 cargo build 必加）
  - Branch refactor/ddd-context-split
  - Backup is 4-LAYER：禁建 src/backup/dao.rs，禁加 pub mod dao 到 mod.rs
  - service/snapshot.rs 禁 import rusqlite、禁 import library::dao
    （僅 library::facade::* 與 library::{Novel, ReadProgress} 允許）
  - 不修改 SQLite schema（C4）；不修改 CLI grammar / config.toml key（C3）
  - 不執行 cargo clean（首次 wreq+boringssl 編譯 ~2-3 min）
  - 不 commit .claude/{wip,analyze,skills}/
  - 不引入 thiserror / domain error；維持 anyhow + Context
  - 不引入 trait NovelRepository 之類抽象（KD 7）
  - TRANSITION marker 強制：任何中間態暫留 re-export 或函數別名加
    `// TRANSITION: removed in task-backup-02 cleanup`

  Rust module 衝突（關鍵實作風險）：
    Rust 不允許 src/backup.rs 與 src/backup/mod.rs 同時存在 — `mod backup;`
    必須恰好對應一個。實作策略二選一：

    策略 A（推薦，乾淨）— 一次性原子搬移：
      將 src/backup.rs 的全部內容拆到 src/backup/ 目錄下：
        - Snapshot types + build_backup/export_to/import_from → service/snapshot.rs
        - push_local/push_webdav/prune_local/backup_filename → service/transport.rs
        - run_backup + BackupReceipt → facade.rs
      然後 `rm src/backup.rs`。一次 commit 內完成；cargo build 過。
      backup-02 變成「驗證 + Conformist grep 自檢 + 補充註解」的清理 task。

    策略 B（嚴格按 task-backup-01 拆兩段）：
      只放 Snapshot 區段到 service/snapshot.rs，transport 段塞到
      src/backup/mod.rs body（暫居）；同時 rm src/backup.rs。
      backup-02 再把 transport 段從 mod.rs body 搬到 service/transport.rs，
      把 run_backup 從 mod.rs body 搬到 facade.rs。
      此策略需要 mod.rs 內容暫時較長 — 加 `// TRANSITION: moved in backup-02` 標記。

    **不可能策略**：保留 src/backup.rs 同時建 src/backup/mod.rs —— Rust
    compile error E0761 "file for module `backup` found at both ...backup.rs
    and ...backup/mod.rs"。

  Library facade naming 對齊（修改 build_backup/import_from 內呼點）：
    原 src/backup.rs 用 Storage 介面 list_novels / get_progress /
    upsert_novel / save_progress；改為 library::facade 對應呼法：
      store.list_novels()      → crate::library::facade::list_shelf(db)
      store.get_progress(id)   → crate::library::facade::get_progress(db, id)
      store.upsert_novel(&n)   → crate::library::facade::add_novel(db, &n)
      store.save_progress(&p)  → crate::library::facade::save_progress(db, &p)
    （library::facade 已存在以上 4 個 wrapper，dead_code allow 中，本 task 會點亮使用）

Files:
  新增：
    - src/backup/mod.rs              # doc + pub mod facade; pub mod service;（無 dao）
    - src/backup/facade.rs           # placeholder（backup-02 填 run_backup）
                                       # 若採策略 A，本檔即放 run_backup + BackupReceipt
    - src/backup/service/mod.rs      # pub mod snapshot; pub mod transport;
    - src/backup/service/snapshot.rs # Backup/BackedUpNovel/ProgressDump/ImportSummary
                                       # + VERSION const
                                       # + build_backup/export_to/import_from
                                       # 全部 storage 操作走 crate::library::facade
    - src/backup/service/transport.rs # placeholder（策略 A：完整 transport 內容）

  修改：
    - src/main.rs                    # `mod backup;` 不變（Rust 自動切到 backup/mod.rs）
                                       # 不需改動，因 `mod backup;` 既能指 .rs 也能指 dir

  刪除：
    - src/backup.rs                  # 因為 src/backup.rs 與 src/backup/mod.rs
                                       # 不能共存，此檔必須刪除（內容已搬走）

  禁建：
    - src/backup/dao.rs              # Conformist 設計刻意不建（驗收標準明列）
```

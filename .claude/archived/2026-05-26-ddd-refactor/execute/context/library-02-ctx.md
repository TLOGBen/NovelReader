# TASK-library-02 Context

task_id: TASK-library-02
task_title: "搬移 DAO 方法 + open_db() factory"
group: library
prerequisites: [TASK-library-01]

Goal: |
  把 src/storage.rs 內屬 Library 的 SQL 方法（novels / chapters.content / progress 相關）
  搬到 src/library/dao.rs；保留 open() factory 作為共享 SQLite connection 入口。
  Catalog / Backup DAO 之後將注入此 connection。
  storage.rs 暫保留為 re-export alias（TRANSITION marker），讓現有 cli.rs / backup.rs
  不爆；下個 group 內逐步切換 import。

  上層目標（goal.md）：按 DDD 藍圖把扁平結構重構為 4 個 bounded context × 5 層架構，
  且不破壞 SQLite schema（C4）、CLI grammar（C3）、book-source JSON 格式（C6）、
  config.toml key、.claude/skills/（C10）等使用者可見契約。

Requirements:
  - REQ-001 (Scen 1.3 — 舊扁平檔案已搬移；本 task 只完成 storage.rs alias，最終刪除在 presentation-02)
  - REQ-002 (Scen 2.3 — DAO 是唯一 import rusqlite 的層；library/dao.rs 是合法 import 點)
  - REQ-003 (Scen 3.2 — SQLite schema 完全不變：sources/novels/chapters/progress 四表 .schema 一致)
  - REQ-004 (Scen 4.1 — cargo test 全綠；舊 DB 仍能讀)
  - REQ-005 (Scen 5.1 — warning 數量 ≤ 2 條 baseline)

Scenarios:
  REQ-001 Scen 1.3:
    Given: 重構完成
    When: 列出 src/ 第一層 .rs 檔
    Then: 只剩 main.rs 與 config.rs（utils 是目錄）；不再有 cli.rs, scraper.rs, storage.rs,
          backup.rs, reader.rs, models.rs
    NOTE: 本 task 階段 storage.rs 暫留為 alias；最終刪除留 task-presentation-02 cleanup。

  REQ-002 Scen 2.3:
    Given: 重構完成
    When: grep -rln "use rusqlite" src/ | grep -vE "(dao|storage)\.rs"
    Then: 無輸出（rusqlite 只在 dao.rs / 過渡期 storage.rs 出現）

  REQ-003 Scen 3.2 (KEY for this task):
    Given: 對重構前 binary 跑過 source import / add / sync 產生資料，
           存在 ~/.local/share/novel-looker/novel-looker.db
    When: 重構後 binary 開啟同 DB 並執行任一查詢
    Then: 不出現「no such column」或 schema mismatch 錯誤
    And: sqlite3 .schema 輸出與重構前完全一致

  REQ-004 Scen 4.1:
    Given: 重構完成
    When: cargo test
    Then: 退出碼為 0；source::rule::tests 4 個案例通過

  REQ-005 Scen 5.1:
    Given: baseline = 2 條 dead_code warning（select_within, extract_all_doc）
    When: LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker
    Then: warning 數量 ≤ 2，無 unused_imports

Task:
  acceptance_criteria:
    - src/library/dao.rs 提供：
      * pub struct LibraryDb { conn: Connection } + pub fn open() -> Result<Self>
      * 方法：upsert_novel, list_novels, get_novel, replace_toc, list_chapters,
        get_chapter, save_chapter_content, save_progress, get_progress
    - 上述方法簽名與舊 Storage::* 等價（return type / 參數 / 錯誤型別一致）
      但按 design.md Borrow 規則表調整 &self / &mut self
    - SCHEMA const 保留全部 4 表（sources / novels / chapters / progress）
      — 不可只保留 Library 三表，Catalog DAO 之後才搬 sources CRUD
    - src/storage.rs 內容改為 TRANSITION alias：
        // TRANSITION: removed in task-presentation-02 cleanup
        //! 過渡別名：所有 Library DAO 重新導出。catalog/backup refactor 完成後刪除此檔。
        pub use crate::library::dao::LibraryDb as Storage;
        pub use crate::library::dao::*;
    - 確認既有 use crate::storage::Storage 仍可用（cli.rs / backup.rs 呼叫不破）
    - cargo build + cargo test 通過
    - E3 schema diff：sqlite3 .schema 對比重構前一致

  steps:
    1. 重寫 library/dao.rs:
       - 從 src/storage.rs 複製 SCHEMA const 與 pub struct Storage { conn: Connection }，
         改名 LibraryDb
       - 複製 Storage::open() 改名 LibraryDb::open()，路徑與錯誤訊息保留
       - 複製 Library 相關方法：upsert_novel, list_novels, get_novel, replace_toc,
         list_chapters, get_chapter, save_chapter_content, save_progress, get_progress
       - 在檔案頂部加 doc comment：
         //! Library DAO. NOTE: Shared Kernel — chapters.{idx,name,url} 由 Catalog DAO 寫；
         //! 本檔僅負責 chapters.content / novels / progress。
         //! sources 表 schema 含於 SCHEMA 但 CRUD 在 catalog/dao.rs。
       - 提供 conn(&self) / conn_mut(&mut self) accessor 方法供日後 Catalog DAO 注入

    2. 保留 storage.rs 別名（含 TRANSITION marker）:
       - src/storage.rs 內容改為：
           // TRANSITION: removed in task-presentation-02 cleanup
           //! 過渡別名：所有 Library DAO 重新導出。catalog/backup refactor 完成後刪除此檔。
           pub use crate::library::dao::LibraryDb as Storage;
           pub use crate::library::dao::*;
       - 確認 use crate::storage::Storage 在 cli.rs / backup.rs 仍可用

    3. 借用簽名按 design.md「Borrow 規則」分 &self vs &mut self（見下 method_signatures 段）

    4. 驗證:
       - LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker (warning ≤ 2)
       - cargo test
       - cargo run -- shelf（書數與 baseline 一致）
       - cargo run -- read $NID 0（內文 > 5 行）
       - sqlite3 ~/.local/share/novel-looker/novel-looker.db .schema 對比 baseline

Design:
  borrow_rules_table: |
    | 操作類型           | 函數簽名                                          |
    |--------------------|---------------------------------------------------|
    | 唯讀 (SELECT)      | fn xxx(db: &LibraryDb, ...) -> Result<T>          |
    | 單一寫入           | fn xxx(db: &mut LibraryDb, ...) -> Result<T>      |
    | Transaction        | fn xxx(db: &mut LibraryDb, ...) -> Result<T>      |

  library_db_interface: |
    pub struct LibraryDb { conn: Connection }
    impl LibraryDb {
        pub fn open() -> Result<Self> { /* ... */ }
        pub fn conn(&self) -> &Connection { &self.conn }
        pub fn conn_mut(&mut self) -> &mut Connection { &mut self.conn }
    }

  shared_kernel_note: |
    在 catalog/dao.rs 與 library/dao.rs 開頭都加：
    //! NOTE: Shared Kernel — sources.* 與 chapters.{idx,name,url} 由 Catalog 寫；
    //! chapters.content 由 Library 寫。修改任一方 schema 需同步檢視對方 DAO。

  db_connection_sharing: |
    LibraryDb 是 SQLite Connection 的 owner。Catalog DAO / Backup DAO 之後將透過
    handle 注入（catalog::dao::*(db: &mut LibraryDb, ...) 簽名借用，不自己 Connection::open）。
    main.rs 才會做一次性 wiring。本 task 不引入 Catalog / Backup DAO，但 LibraryDb
    必須暴露 conn_mut() 供後續 task 注入。

  schema_const_must_keep_all_4_tables: |
    SCHEMA const 必須完整包含：sources / novels / chapters / progress
    （不可只保留 Library 三表）— 否則 Catalog refactor 完成前 schema 會缺 sources 表，
    E3 schema diff 會 fail。Catalog 的 sources CRUD 留 TASK-catalog 群組搬。

  app_context_rule: |
    日後 AppContext { pub db: LibraryDb, ... } by value；handler 統一收 &mut AppContext
    （read 也用 &mut 給統一性）。本 task 不寫 AppContext，但 LibraryDb 設計要相容。

Test:
  E1_compile_test: |
    每個 step 後跑：
    - LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo check
    - LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker
    - cargo test（source::rule::tests 4 案例必過）

  E3_schema_diff (MANDATORY, key acceptance for this task): |
    Before refactor (已在 setup baseline)：
      sqlite3 ~/.local/share/novel-looker/novel-looker.db .schema > /tmp/schema-baseline.txt
    After this task:
      sqlite3 ~/.local/share/novel-looker/novel-looker.db .schema > /tmp/schema-after.txt
      diff /tmp/schema-baseline.txt /tmp/schema-after.txt   # 必須無輸出

  E7_subset (本 step 對應)：
    cargo run -- shelf            # 書數與 baseline 一致
    cargo run -- read $NID 0      # 內文行數 > 5

  integration_check (Shared Kernel 寫入欄位分工): |
    本 task 還未拆 Catalog DAO，所以 chapters.{idx,name,url} 與 chapters.content 仍由
    同一個 LibraryDb 寫；只要 sync + read 流程行為與 baseline 一致即通過。

Constraints:
  read_only_areas:
    - .claude/analyze/    # 唯讀，禁止修改
    - .claude/skills/     # 禁止任何改動（REQ-006）
    - .claude/wip/        # 禁止改動
    - book-sources/       # JSON 格式不變（REQ-003 Scen 3.3）
    - SQLite schema       # 任何欄位增刪 / index 變動 / rename 都禁止（C4, OUT OF SCOPE）

  must_keep_unchanged:
    - SCHEMA const 內容（4 表的 CREATE TABLE 字串）
    - LibraryDb::open() 的 DB 檔案路徑（data_dir().join("novel-looker.db")）
    - 錯誤訊息文字（"open sqlite {}", path.display()）
    - novels.book_url 仍為 UNIQUE 自然 key
    - replace_toc 仍用 transaction（DELETE + INSERT）
    - 方法 return type 與 anyhow::Result<T> 簽名等價

  forbidden:
    - 不可刪除 sources 表 schema（Catalog 還沒搬）
    - 不可改變 schema CREATE TABLE 字串裡任何空白 / 欄位順序（會被 E3 diff 抓）
    - 不可在 library/dao.rs 引入新 error type（KD 7 — 不過早抽象，仍用 anyhow）
    - 不可在 src/storage.rs 留下任何 SQL 邏輯（必須全 re-export，沒有自己的 impl）
    - 不可執行 cargo clean
    - 不可在 main 分支直接 commit（用 refactor/ddd-context-split branch）

  baseline:
    warning_count_max: 2          # baseline: select_within + extract_all_doc dead_code
    libclang_path: "/usr/lib/llvm-18/lib"   # 所有 cargo build / run 命令必須前綴
    branch: "refactor/ddd-context-split"

  transition_markers:
    - 所有暫時別名 / 過渡 re-export 必須標 `// TRANSITION: removed in <task-id> cleanup`
    - 本 task 在 src/storage.rs 留下 TRANSITION marker，由 task-presentation-02 清除
    - 最終 grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/ 必須無輸出
      （非本 task 的 gate，是最終 PR gate；本 task 只負責正確加上 TRANSITION 註解）

method_signatures:
  source_file: src/storage.rs (current Storage struct)
  target_file: src/library/dao.rs (new LibraryDb struct)

  schema_const: |
    複製整段 SCHEMA const 字串（lines 8-47 of storage.rs），維持原樣，包含全 4 表
    (sources / novels / chapters / progress)。

  open: |
    current:  pub fn open() -> Result<Self>
    new:      pub fn open() -> Result<Self>     # 不變，只是 self type 改為 LibraryDb
    body:     完全保留（data_dir → join → create_dir_all → Connection::open
              → execute_batch(SCHEMA)）

  library_methods_to_move:
    upsert_novel:
      current_sig: "pub fn upsert_novel(&self, n: &Novel) -> Result<i64>"
      new_sig:     "pub fn upsert_novel(&mut self, n: &Novel) -> Result<i64>"
      rationale:   "design.md Borrow 規則：INSERT/UPDATE 屬單一寫入 → &mut self"

    list_novels:
      current_sig: "pub fn list_novels(&self) -> Result<Vec<Novel>>"
      new_sig:     "pub fn list_novels(&self) -> Result<Vec<Novel>>"
      rationale:   "SELECT 唯讀 → &self（不變）"

    get_novel:
      current_sig: "pub fn get_novel(&self, id: i64) -> Result<Option<Novel>>"
      new_sig:     "pub fn get_novel(&self, id: i64) -> Result<Option<Novel>>"
      rationale:   "SELECT 唯讀 → &self（不變）"

    replace_toc:
      current_sig: "pub fn replace_toc(&mut self, novel_id: i64, chapters: &[ChapterMeta]) -> Result<()>"
      new_sig:     "pub fn replace_toc(&mut self, novel_id: i64, chapters: &[ChapterMeta]) -> Result<()>"
      rationale:   "Transaction → &mut self（本來就是 &mut self，不變）"

    list_chapters:
      current_sig: "pub fn list_chapters(&self, novel_id: i64) -> Result<Vec<ChapterMeta>>"
      new_sig:     "pub fn list_chapters(&self, novel_id: i64) -> Result<Vec<ChapterMeta>>"
      rationale:   "SELECT 唯讀 → &self（不變）"

    get_chapter:
      current_sig: "pub fn get_chapter(&self, novel_id: i64, idx: i64) -> Result<Option<Chapter>>"
      new_sig:     "pub fn get_chapter(&self, novel_id: i64, idx: i64) -> Result<Option<Chapter>>"
      rationale:   "SELECT 唯讀 → &self（不變）"

    save_chapter_content:
      current_sig: "pub fn save_chapter_content(&self, novel_id: i64, idx: i64, content: &str) -> Result<()>"
      new_sig:     "pub fn save_chapter_content(&mut self, novel_id: i64, idx: i64, content: &str) -> Result<()>"
      rationale:   "UPDATE 單一寫入 → &mut self（從 &self 改為 &mut self；
                    呼叫端 storage.rs 別名生效後仍可用，但 caller 須持 &mut Storage）"

    save_progress:
      current_sig: "pub fn save_progress(&self, p: &ReadProgress) -> Result<()>"
      new_sig:     "pub fn save_progress(&mut self, p: &ReadProgress) -> Result<()>"
      rationale:   "INSERT/UPDATE 單一寫入 → &mut self"

    get_progress:
      current_sig: "pub fn get_progress(&self, novel_id: i64) -> Result<Option<ReadProgress>>"
      new_sig:     "pub fn get_progress(&self, novel_id: i64) -> Result<Option<ReadProgress>>"
      rationale:   "SELECT 唯讀 → &self（不變）"

  data_dir_helper: |
    fn data_dir() -> Result<PathBuf>   # 從 storage.rs 一併搬入 library/dao.rs，
                                       # 留 private fn，由 LibraryDb::open() 內部呼

  sources_methods_handling: |
    NOTE: 按 task 步驟原文，只列出 9 個 Library 方法要搬。
    save_source / list_sources / get_source 三方法的歸屬：
    - task-library.md 沒有明確指示這 3 個方法在本 task 怎麼處理
    - 但驗收標準寫 "src/storage.rs 暫保留為 pub use crate::library::dao::*;"
      若 storage.rs 變成純 re-export，sources 方法的 impl 必須有歸宿
    - 唯一合理的解讀：本 task 也將 save_source/list_sources/get_source 一併搬到
      library/dao.rs（暫時掛在 LibraryDb 上），加 doc comment 標註 "Catalog-temp;
      TASK-catalog-03 將拉到 catalog/dao.rs"
    - 它們的借用規則：
      * save_source(&self, ...) → save_source(&mut self, ...)  # 單一寫入
      * list_sources(&self) → list_sources(&self)              # 唯讀，不變
      * get_source(&self, url) → get_source(&self, url)        # 唯讀，不變
    - 若 caller 是 cli.rs 持 &Storage，從 &self → &mut self 會改變借用，
      需要同步調整 caller（cli.rs / backup.rs 內所有 storage 變數宣告為 mut）
    建議：實作時遇到借用衝突，把 cli.rs 內 `let storage = ...` 改成
    `let mut storage = ...`；不破壞功能，只是 mutability annotation。

  storage_rs_final_content: |
    // TRANSITION: removed in task-presentation-02 cleanup
    //! 過渡別名：所有 Library DAO 重新導出。catalog/backup refactor 完成後刪除此檔。
    pub use crate::library::dao::LibraryDb as Storage;
    pub use crate::library::dao::*;

Files:
  to_modify:
    - src/library/dao.rs                    # 從佔位變成完整 LibraryDb 實作
    - src/storage.rs                        # 改為 TRANSITION re-export alias

  may_need_minor_adjustment:
    - src/cli.rs                            # 若 &self → &mut self 改動使呼叫端缺 mut
    - src/backup.rs                         # 同上，可能需要 let mut storage

  must_not_touch:
    - src/library/mod.rs                    # TASK-library-01 已搬完 type 定義
    - src/main.rs                           # AppContext wiring 留 later task
    - src/library/facade.rs                 # TASK-library-03 處理
    - src/library/service/                  # TASK-library-03 處理
    - src/source/                           # TASK-catalog 群組處理
    - src/scraper.rs                        # TASK-catalog 群組處理
    - src/models.rs                         # TASK-library-01 已處理 4 個 type；
                                            # SearchHit 留 catalog group
    - book-sources/*                        # JSON 格式不變
    - .claude/skills/                       # REQ-006 — 絕對不動

  verification_artifacts:
    - /tmp/schema-baseline.txt              # setup 階段產出，本 task 必須與之 diff 對齊
    - /tmp/build-baseline.log               # warning count baseline = 2
    - /tmp/help-baseline.txt                # CLI grammar baseline（本 task 不直接驗，留 presentation-04）

Execution Notes:
  - 所有 cargo 命令前綴：LIBCLANG_PATH=/usr/lib/llvm-18/lib
  - branch: refactor/ddd-context-split
  - 禁止 cargo clean
  - 禁止觸碰 .claude/skills/
  - 完成後必跑 E3 schema diff，這是本 task 最關鍵驗證點
  - storage.rs 改 alias 後，cli.rs / backup.rs 仍透過 `crate::storage::Storage` import 即可
    （Storage = LibraryDb 透過 pub use 別名）；若有 &self → &mut self 借用衝突，
    在呼叫端加 mut 修飾即可，無需改 API surface

task_id: TASK-catalog-02
group: catalog

Goal: |
  把 novel-looker 從按技術分類的扁平結構重構為按 4 個 bounded context
  (Catalog / Library / Backup / Presentation) × 5 層架構組織的目錄結構，
  且不破壞任何使用者可見的對外介面 (CLI grammar / SQLite schema /
  book-source JSON 格式 / config.toml key / .claude/skills/ 對 CLI 相依)。

  本 task 為 catalog group 的第 2 步：把 Scraper 與 SearchHit (Catalog
  bounded context 的對外 PL — Published Language) 從 src/scraper.rs /
  src/models.rs 搬到 src/catalog/ 樹下，刪除這兩個舊檔。
  Cloudflare bypass (Emulation::Chrome131) 路徑必須保留。

Requirements:
  - id: REQ-001
    title: 目錄結構符合 DDD 藍圖
    note: |
      重構後 src/ 下 4 個 bounded context 目錄 + utils/，根目錄只剩
      main.rs / config.rs / utils 入口。Scenario 1.3 為本 task 直接相關。
  - id: REQ-004
    title: 既有功能行為等價
    note: |
      Cloudflare bypass 仍生效 (Scenario 4.2) — 本 task 搬移 Scraper 時
      最大風險點，必須保留 wreq::Client::builder().emulation(Emulation::Chrome131)。
  - id: REQ-005
    title: 編譯品質不退化
    note: |
      重構後編譯不出現新 warning；baseline = 2 條 dead_code warning
      (select_within, extract_all_doc)。本 task 後 ≤ 2。

Scenarios:
  - id: REQ-001 Scen 1.3
    name: 舊扁平檔案已搬移
    given: 重構完成
    when: 列出 src/ 第一層 .rs 檔
    then: |
      只剩 main.rs 與 config.rs (utils 是目錄)；不再有 cli.rs, scraper.rs,
      storage.rs, backup.rs, reader.rs, models.rs
    relevance: |
      本 task 負責消除 src/scraper.rs 與 src/models.rs 兩個檔案。
  - id: REQ-004 Scen 4.2
    name: Cloudflare bypass 仍生效
    given: 重構完成 + 已 import uukanshu 書源
    when: cargo run -- add --source https://uukanshu.cc https://uukanshu.cc/book/21940/
    then: 輸出「✓ 加入書架 (#N) 超維術士 / 佚名」(書名 / 作者非 Unknown)
  - id: REQ-005 Scen 5.1
    name: 編譯 warning 不增加
    given: 基線 2 條 dead_code warning
    when: LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker
    then: warning 數量 ≤ 2；無 unused_imports
  - id: REQ-005 Scen 5.2
    name: 無 cargo check error
    given: 重構任一階段完成
    when: cargo check
    then: 退出碼為 0

Task:
  title: 搬 Scraper + SearchHit (Catalog PL)
  prerequisite: catalog-01 已建立 src/catalog/{mod.rs, service/{mod.rs, source.rs, rule.rs}}
  acceptance:
    - src/catalog/service/scraper.rs 含完整 Scraper (HTTP + 套規則的 search/fetch_info/fetch_toc/fetch_content)
    - Scraper::new() 仍用 wreq::Client::builder().emulation(Emulation::Chrome131) (Cloudflare bypass 不被破壞)
    - SearchHit 在 src/catalog/mod.rs 內定義或 pub use，明確標為 PL (doc comment)
    - src/models.rs 內容空 (全搬走 — Novel 等已在 library group 搬走，SearchHit 此 task 搬走)，整個檔案刪除
    - src/scraper.rs 刪除
    - cargo build + test 通過
  steps:
    - section: 搬 Scraper
      items:
        - 把 src/scraper.rs 內容貼到 src/catalog/service/scraper.rs (resolve 已搬到 utils；normalize_paragraphs 留在新 scraper.rs 為私有 helper — 單一 caller，不搬到 utils)
        - src/main.rs 移除 `mod scraper;`
    - section: 搬 SearchHit
      items:
        - 從 src/models.rs 剪 SearchHit 定義
        - 貼到 src/catalog/mod.rs，加 doc comment：`/// PL: Catalog 對外發佈的搜尋結果型別。Published Language across context boundary.`
        - rm src/models.rs，src/main.rs 移除 `mod models;`
    - section: 更新引用
      items:
        - "grep -rn 'use crate::scraper::' src/：改為 `use crate::catalog::service::scraper::Scraper`"
        - "grep -rn 'use crate::models::SearchHit' src/：改為 `use crate::catalog::SearchHit`"
        - src/catalog/service/mod.rs 加 `pub mod scraper;`
    - section: 驗證
      items:
        - cargo build
        - cargo run -- search "alice" (Gutenberg) 回傳 ≥ 5 筆
        - cargo run -- add --source https://uukanshu.cc https://uukanshu.cc/book/21940/ 印「✓ 加入書架 (#N) 超維術士 / 佚名」(驗證 wreq + Chrome 131 仍生效)

Design:
  type_placement:
    - type: Scraper
      from: src/scraper.rs
      to: src/catalog/service/scraper.rs
      reason: Catalog service
    - type: SearchHit
      from: src/models.rs
      to: src/catalog/mod.rs (pub re-export 為 PL)
      reason: Catalog 對外 PL — Published Language across context boundary
  module_layout: |
    catalog/
    ├── mod.rs               # //! Catalog: 描述如何從網站抽資料並執行抽取
    │                        # 含 SearchHit (PL re-export)；已有 pub use service::source::BookSource
    ├── facade.rs            # (catalog-03 才建)
    ├── dao.rs               # (catalog-03 才建)
    └── service/
        ├── mod.rs           # pub mod source; pub mod rule; pub mod scraper;
        ├── source.rs        # BookSource (catalog-01 已搬)
        ├── rule.rs          # rule DSL (catalog-01 已搬)
        └── scraper.rs       # Scraper (本 task 搬)
  scraper_invariants_preserved: |
    - HTTP client 是 wreq::Client 配 Emulation::Chrome131 — Cloudflare bypass
      的 TLS / JA3 / JA4 / HTTP-2 指紋。不可換成 reqwest 或拿掉 emulation。
    - 所有相對 URL 用 resp.uri() (wreq API) resolve 對 final URL (after redirects)
    - fetch_content 後處理 normalize_paragraphs；用 extract_all_doc (非 extract_doc)
    - BookSource.header JSON headers 套到每個請求，無效 JSON 靜默忽略
  shared_kernel_note: 本 task 不涉及 dao；scraper 是純 service 層 (HTTP + rule 套用)，不接 SQL

Test:
  e2e_subset:
    - id: E1
      cmd: cargo test
      expect: 全綠 (source::rule::tests 4 個案例通過，現為 catalog::service::rule::tests)
    - id: E6
      cmd: cargo run -- add --source https://uukanshu.cc https://uukanshu.cc/book/21940/
      expect: 印「✓ 加入書架 (#N) 超維術士 / 佚名」(Cloudflare bypass 驗證)
    - id: Gutenberg search
      cmd: cargo run -- search "alice"
      expect: ≥ 5 筆結果 (HTTP path 驗證)
  boundary_checks:
    - name: Cloudflare bypass 路徑保留
      cmd: grep -rn "Emulation::Chrome131" src/catalog/service/scraper.rs
      expect: 恰有 1 行
    - name: 編譯零 new warning
      cmd: LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker
      expect: warning ≤ 2 (baseline = 2 dead_code)
    - name: 無 unused_imports
      cmd: cargo build
      expect: 不出現 unused_imports warning
    - name: 舊 scraper import 全清
      cmd: grep -rn "use crate::scraper::" src/
      expect: 無輸出
    - name: 舊 models import 全清
      cmd: grep -rn "use crate::models::" src/
      expect: 無輸出
    - name: 舊檔案已刪
      cmd: ls src/scraper.rs src/models.rs 2>&1
      expect: "No such file or directory"

Constraints:
  build_env:
    - LIBCLANG_PATH=/usr/lib/llvm-18/lib (Ubuntu 24.04；wreq 依 BoringSSL 經 boring crate 需 bindgen → libclang)
  warning_baseline: 2 (dead_code: select_within, extract_all_doc)
  branch: refactor/ddd-context-split
  forbidden_changes:
    - 不修改 SQLite schema (本 task 本就不碰 dao，但通則)
    - 不修改 CLI grammar / config.toml key / book-source JSON 欄位
    - 不修改 .claude/skills/ 任何檔案
    - 不修改 library/* 內部 (library/dao.rs 不 import Scraper 或 SearchHit，應完全不動)
    - 不改 wreq 為 reqwest；不拿掉 Emulation::Chrome131
    - 不把 normalize_paragraphs 搬到 utils (單一 caller — 規則：第二個 caller 出現才提升)
  transition_markers: |
    若有暫時別名/過渡 re-export，強制用 `// TRANSITION:` 註解標示
    並於 task 完成時清零。最終 PR 前以下 grep 必須空：
    grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/
  layer_invariants:
    - Service 層不 import rusqlite (catalog/service/scraper.rs 不會碰 SQL，本來就符合)
    - Service 層不 import 任何 dao module
  shared_kernel: 本 task 不涉 Shared Kernel 寫入 (chapters TOC writes 留待 catalog-03)

Files:
  new_or_replace:
    - path: src/catalog/service/scraper.rs
      action: create (內容來自 src/scraper.rs)
      content_outline: |
        use anyhow::{anyhow, Result};
        use scraper::Html;
        use wreq::Client;
        use wreq_util::Emulation;
        use crate::library::{ChapterMeta, Novel};
        use crate::catalog::SearchHit;            # 新路徑
        use crate::catalog::service::rule;         # 已在 catalog-01 搬好
        use crate::catalog::BookSource;            # pub use 於 catalog/mod.rs (catalog-01)

        pub struct Scraper { client: Client }
        impl Scraper {
            pub fn new() -> Result<Self> {
                let client = Client::builder()
                    .emulation(Emulation::Chrome131)  # MUST 保留
                    .build()?;
                Ok(Self { client })
            }
            async fn fetch(...) { ... }
            pub async fn search(...) -> Result<Vec<SearchHit>> { ... }
            pub async fn fetch_info(...) -> Result<Novel> { ... }
            pub async fn fetch_toc(...) -> Result<Vec<ChapterMeta>> { ... }
            pub async fn fetch_content(...) -> Result<String> { ... }
        }
        fn normalize_paragraphs(html: &str) -> String { ... }  # 私有 helper，留在此
    - path: src/catalog/mod.rs
      action: edit (新增 SearchHit 定義 + PL doc comment)
      add_content_outline: |
        /// PL: Catalog 對外發佈的搜尋結果型別。Published Language across context boundary.
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct SearchHit {
            // 從 src/models.rs 剪過來的欄位 (name / author / book_url / kind / intro 等)
        }
        # 現有：pub mod service; pub mod dao; pub mod facade;
        # 現有：pub use service::source::BookSource;
    - path: src/catalog/service/mod.rs
      action: edit (新增 `pub mod scraper;`)
      current: "pub mod source; pub mod rule;"
      after:   "pub mod source; pub mod rule; pub mod scraper;"
    - path: src/main.rs
      action: edit (移除 `mod scraper;` 與 `mod models;`)
  delete:
    - src/scraper.rs
    - src/models.rs
  update_imports:
    - path: src/cli.rs
      change: |
        use crate::scraper::Scraper      → use crate::catalog::service::scraper::Scraper
        use crate::models::SearchHit (若有) → use crate::catalog::SearchHit
    - path: src/reader.rs
      change: |
        use crate::scraper::Scraper      → use crate::catalog::service::scraper::Scraper
  untouched:
    - src/library/*  (library/dao.rs 不 import Scraper 或 SearchHit；確認無變動)
    - .claude/skills/*  (REQ-006)
    - book-sources/*.json (REQ-003 Scen 3.3)
  preserved_internal_dependencies:
    - src/catalog/service/scraper.rs imports `crate::catalog::service::rule` (post-catalog-01 path — 已就位)
    - src/catalog/service/scraper.rs imports `crate::catalog::BookSource` (re-export from catalog/mod.rs — 已就位)

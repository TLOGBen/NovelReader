Goal: |
  按 DDD 藍圖把 novel-looker 從扁平結構重構為 4 個 bounded context (Catalog / Library / Backup /
  Presentation) × 5 層架構 (action / facade / service / DAO / utils) 的目錄結構，且不破壞使用者可見對外介面。

  本 task (TASK-shared-01) 是 shared 群組第一步：建立 `src/utils/` 骨架並把 `scraper.rs` 內的
  `resolve()` helper 搬出，作為 utils pool 的第一個成員。對應 goal 範圍 In-scope:
  「拆出 `src/utils/` 並至少搬一個共用 helper 進去（按 OQ-4 候選名單擇一）」。

  相關驗收標準：
  - C1 編譯通過：`LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker` 無新 warning
  - C2 `cargo test` 全綠（特別是 `source::rule::tests` 4 個案例）
  - C8 目錄結構符合 DDD 藍圖（`src/utils/` 存在，即使只有一個 helper）

Requirements:
  REQ-001:
    描述: |
      重構後 `src/` 下出現 4 個 bounded context 目錄（catalog/ library/ backup/ presentation/）+ utils/，
      每個 context 內按需要含 dao.rs、service/、facade.rs；既有檔案按 `.claude/wip/ddd-analysis.md` §4
      File-to-Context Mapping 全數搬完，根目錄只剩 main.rs / config.rs / utils 入口。
    本 task 相關: 此 task 只負責建立 `src/utils/` 子集（mod.rs + url.rs），其他 context 目錄不在 shared-01 範圍。
  REQ-005:
    描述: |
      重構後編譯不出現新 warning；既有 warning（如 `select_within` dead_code）數量不增加。
      基線 warning 數量 = 2 (dead_code: select_within, extract_all_doc)。

Scenarios:
  # REQ-001 相關 (僅 utils 部分)
  Scenario_1.1_目錄結構檢查:
    Given: 重構完成的 codebase
    When: 執行 `find src -type d -mindepth 1 -maxdepth 1 | sort`
    Then: 輸出包含 `src/utils` (本 task 範圍：只需建立 utils 目錄)

  # REQ-005 相關
  Scenario_5.1_編譯_warning_不增加:
    Given: 重構前 `cargo build --bin novel-looker` 產生 2 條 dead_code warning
    When: 重構後 `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker`
    Then: warning 數量 ≤ 2
    And: 無 unused_imports、no_use 之類 module 搬移留下的痕跡
  Scenario_5.2_無_cargo_check_error:
    Given: 重構任一階段完成（每個 step 後）
    When: 執行 `cargo check`
    Then: 退出碼為 0

Task:
  id: TASK-shared-01
  title: 建立 utils/ 骨架 + 搬出 resolve() helper
  前置群組: 無
  目標: |
    `src/utils/` 目錄存在，至少包含一個 helper (resolve URL)，main.rs 已宣告 mod；
    其餘程式碼透過 `crate::utils::url::resolve` 引用，原 `scraper.rs::resolve` 移除。

  驗收標準:
    - "`src/utils/mod.rs` 與 `src/utils/url.rs` 存在"
    - "`src/utils/url.rs` 包含從原 `src/scraper.rs` 搬出的 `pub fn resolve(base: &str, href: &str) -> Result<String>`"
    - "`src/scraper.rs` 內 `resolve` 私有函數已移除，呼叫處改用 `crate::utils::url::resolve`"
    - "`cargo build --bin novel-looker`（含 `LIBCLANG_PATH`）通過，無新 warning"
    - "`cargo test` 全綠"

  步驟:
    建立_utils_模組:
      - "`mkdir -p src/utils/`"
      - "建立 `src/utils/mod.rs`，內容只含 `pub mod url;`"
      - "建立 `src/utils/url.rs`，從 `src/scraper.rs` 複製 `resolve` 函數體（含 `use anyhow::{Context, Result};` 與 `use url::Url;`），改為 `pub fn resolve(...)`"
    更新引用:
      - "`src/main.rs` 加上 `mod utils;`（在 alphabetical 順序適當位置）"
      - "`src/scraper.rs` 刪除原 `fn resolve(...)` 定義"
      - "`src/scraper.rs` 把全部 `resolve(...)` 呼叫（grep 應為 5 處 call site + 1 處 fn 定義）改為 `crate::utils::url::resolve(...)`，原 fn 定義刪除"
    驗證:
      - "LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker"
      - "cargo test"
      - "cargo run -- search \"alice\"（Gutenberg 走 URL resolve 路徑）回傳 ≥ 5 筆結果"

Design:
  目錄藍圖_utils_片段: |
    src/
    ├── main.rs                  # 入口 + module 宣告 + tokio runtime
    ├── config.rs                # 跨 context 共用設定（root infra）
    │
    ├── utils/
    │   ├── mod.rs
    │   └── url.rs               # resolve()（從 scraper.rs 搬出；第一個成員）

  型別放置_相關行: |
    | Type | 從 | 搬至 | 原因 |
    | `resolve` helper (URL) | `src/scraper.rs` | `src/utils/url.rs` | 跨 context 共用 helper (utils pool 第一成員) |
    | `Config` + sub-config | `src/config.rs` | **不動，保留 root** | 跨 context 共用 infra |

  Verification_中間態可編譯保證: |
    `.claude/wip/ddd-analysis.md` §5 Refactor Roadmap 每步要求「保持中間態可編譯可跑」。本 task
    是 shared 群組第一步，搬完後 `cargo build` + `cargo test` 必須直接通過（不需 TRANSITION
    別名，因為 resolve 是無相依的純函數）。

  TRANSITION_marker_convention: |
    refactor 過程中所有暫時別名、過渡 re-export、暫留檔案，強制標 `// TRANSITION:` 註解
    （含拆完移除的 task 編號）。本 task 若不需暫時別名（純搬移），不應引入 TRANSITION marker。
    最終 PR 前以下 grep 必須空輸出：
      grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/ 2>/dev/null

Test:
  E1_編譯_單元測試:
    起點: 在 refactor branch 上
    終點: cargo test 全綠
    對應: C1, C2, REQ-004 Scen 4.1, REQ-005
    中間態跑: 每 step 後跑（本 task 完成後必跑）

  關鍵邊界條件_本_task_適用:
    編譯零_new_warning:
      驗證: 基線 cargo build warning 數量 = 2 (dead_code: select_within, extract_all_doc)；重構後 ≤ 2
    Mod_路徑變動不留_dead_use:
      驗證: cargo build 不出現 unused_imports
    TRANSITION_殘留清零:
      驗證: 'grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/ 2>/dev/null 無輸出'

  驗證執行順序:
    - "1. cargo check（必過）"
    - "2. LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker（必過、warning 不增加）"
    - "3. cargo test（必綠）"
    - "4. 跑該 step 對應的 e2e 子集（本 task: E1 + 步驟「驗證」段中 cargo run -- search \"alice\"）"
    - "5. 跑該 step 對應的整合測試自檢（grep 類；本 task 無 service/dao grep 適用）"

Constraints:
  - "所有 cargo 命令必須前綴 `LIBCLANG_PATH=/usr/lib/llvm-18/lib`（wreq 透過 boring → bindgen → libclang）"
  - "工作分支：refactor/ddd-context-split"
  - "`.claude/skills/` 一個字元都不可改動（REQ-006 Scen 6.1）"
  - "TRANSITION marker convention：任何過渡別名/re-export 強制標 `// TRANSITION: removed in <task-id> cleanup`；最終 grep 必須清零"
  - "不修改 SQLite schema（out of scope，本 task 無 DB 操作）"
  - "不修改 CLI grammar / config.toml key / book-source JSON 欄位"
  - "不新增 feature 測試（pure refactor，回歸 + 結構自檢為主）"
  - "讀寫前必 Read（rules/read-before-write.md）"
  - "原 `scraper.rs::resolve` 為私有函數；搬到 utils 後改為 `pub fn`"
  - "保留原函數的 use clause 完整性：`use anyhow::{Context, Result};` 與 `use url::Url;`"
  - "grep 預期：scraper.rs 內 `resolve` 應有 5 處 call site + 1 處 fn 定義，全部要改/刪"
  - "main.rs 加 `mod utils;` 須維持 alphabetical 順序"
  - "本 task 為純搬移，理論上不需引入 TRANSITION 別名"

Files:
  新增:
    - path: src/utils/mod.rs
      內容: "只含 `pub mod url;`"
    - path: src/utils/url.rs
      內容: |
        從 src/scraper.rs 複製 resolve 函數體，含：
        - use anyhow::{Context, Result};
        - use url::Url;
        - pub fn resolve(base: &str, href: &str) -> Result<String>
  修改:
    - path: src/main.rs
      變更: 加上 `mod utils;` 宣告（alphabetical 順序適當位置）
    - path: src/scraper.rs
      變更: |
        - 刪除原 `fn resolve(...)` 定義
        - 將全部 5 處 `resolve(...)` call site 改為 `crate::utils::url::resolve(...)`
  驗證指令:
    - "LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker"
    - "cargo test"
    - "cargo run -- search \"alice\"   # Gutenberg 走 URL resolve 路徑，預期 ≥ 5 筆結果"
    - "grep -rnE \"_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:\" src/   # 必須空輸出"

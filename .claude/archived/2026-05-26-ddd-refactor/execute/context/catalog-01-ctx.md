Goal: |
  按 DDD 藍圖將 novel-looker 從扁平 src/ 結構重構為按 4 個 bounded context（Catalog / Library / Backup / Presentation）× 5 層架構組織。
  本 task 屬於 catalog group 第一步：建立 src/catalog/ 目錄骨架，把 BookSource struct 與 Rule DSL 從 src/source/{mod.rs, rule.rs} 搬入 src/catalog/service/{source.rs, rule.rs}，刪除 src/source/，更新所有 import 路徑。
  約束：不破壞 CLI grammar、SQLite schema、book-source JSON 格式、config.toml key、.claude/skills/ 對 CLI 的相依。

Requirements:
  REQ-001:
    description: |
      重構後 src/ 下出現 4 個 bounded context 目錄 + utils/，
      每個 context 內按需要含 dao.rs、service/、facade.rs；
      既有檔案按 wip/ddd-analysis.md §4 File-to-Context Mapping 全數搬完。
    relevant_scenarios: [1.1, 1.2, 1.3]
  REQ-002:
    description: |
      每個 context 內 service 層的 .rs 檔不直接 import SQL 相關 crate (rusqlite)
      也不 import 任何 dao module；DAO 是唯一接觸 SQL 的層；
      facade 是唯一同時呼叫 service 與 DAO 的層。
    relevant_scenarios: [2.1, 2.2]
  REQ-005:
    description: |
      重構後編譯不出現新 warning；既有 warning（如 select_within dead_code）數量不增加。
      基線：2 條 dead_code warning（select_within + extract_all_doc）。

Scenarios:
  - id: 1.1
    name: 目錄結構檢查
    given: 重構完成的 codebase
    when: 執行 `find src -type d -mindepth 1 -maxdepth 1 | sort`
    then: 輸出包含 src/catalog（含其他 context），且不包含 src/source（已搬入 catalog）
  - id: 1.2
    name: Context mod.rs 存在且註解職責
    given: 4 個 context 目錄都建立
    when: Read src/catalog/mod.rs
    then: 檔案最上方有 //! doc comment 描述該 context 的 Purpose 與對外 PL
  - id: 1.3
    name: 舊扁平檔案已搬移
    given: 重構完成
    when: 列出 src/ 第一層 .rs 檔
    then: 不再有 src/source/ 目錄
  - id: 2.1
    name: service 不 import rusqlite
    given: 重構完成
    when: 執行 `grep -rn "use rusqlite" src/*/service/ 2>/dev/null`
    then: 無輸出
  - id: 2.2
    name: service 不 import 任何 dao module
    given: 重構完成
    when: 執行 `grep -rnE "use crate::[a-z]+::dao" src/*/service/ 2>/dev/null`
    then: 無輸出

Task:
  id: TASK-catalog-01
  title: 建立 catalog/ 結構 + 搬 BookSource 與 Rule DSL
  prereq_group: library
  objective: |
    src/catalog/{mod.rs, service/{mod.rs, source.rs, rule.rs}} 建立；
    src/source/{mod.rs, rule.rs} 內容搬入；
    既有 `cargo test source::rule::tests` 4 個案例改 path 後仍通過（新路徑 catalog::service::rule::tests）。
  acceptance:
    - src/catalog/mod.rs 含 `//! Catalog: 描述如何從網站抽資料並執行抽取` doc + Outbound PL 列表（SearchHit / Novel / Vec<ChapterMeta> / RawContent）
    - catalog/service/source.rs 含 BookSource 及所有 sub-rule struct（從 src/source/mod.rs 搬）
    - catalog/service/rule.rs 含 rule DSL（從 src/source/rule.rs 搬，含 4 個 #[test]）
    - src/source/ 目錄刪除
    - cargo test 4 個 rule tests 通過（新 path catalog::service::rule::tests）
  steps:
    establish_module:
      - mkdir -p src/catalog/service
      - 寫 src/catalog/mod.rs：doc comment + `pub mod service; pub mod dao; pub mod facade;`（後兩個下 task 會填）
      - 寫 src/catalog/service/mod.rs：`pub mod source; pub mod rule; pub mod scraper;`（scraper 下 task 搬）
    move_code:
      - 把 src/source/mod.rs 整個內容貼到 src/catalog/service/source.rs，**移除 `pub mod rule;` 那行**（rule 改到 service/mod.rs 註冊）
      - 把 src/source/rule.rs 整個內容貼到 src/catalog/service/rule.rs，**包含 #[cfg(test)] mod tests 區塊**
      - rm -rf src/source/
      - src/main.rs 移除 `mod source;`，加 `mod catalog;`
    update_imports:
      - grep -rn "use crate::source::" src/，把所有引用改為 `use crate::catalog::service::source::*` 或 `use crate::catalog::service::rule::*`
      - 注意 BookSource 仍要被 storage.rs / cli.rs / backup.rs 看見：在 src/catalog/mod.rs 內 re-export：`pub use service::source::BookSource;`
    verify:
      - cargo build
      - cargo test catalog::service::rule::tests（4 個 case 通過）

Design:
  type_placement:
    BookSource_and_sub_rule_struct:
      from: src/source/mod.rs
      to: src/catalog/service/source.rs
      reason: Catalog 概念
    Rule_RuleAlt_Accessor:
      from: src/source/rule.rs
      to: src/catalog/service/rule.rs
      reason: Catalog service 內部
  catalog_module_purpose: |
    src/catalog/mod.rs 必須有 `//! Catalog: 描述如何從網站抽資料並執行抽取` doc comment。
    對外 PL 列表（Outbound）：SearchHit / Novel / Vec<ChapterMeta> / RawContent。
    BookSource 透過 `pub use service::source::BookSource;` re-export 給其他 context 使用。
  transition_marker_rule: |
    所有暫時別名、過渡 re-export、暫留檔案，強制標 `// TRANSITION:` 註解（含拆完移除的 task 編號）。
    本 task 若需暫時 re-export 給其他 caller 看見 BookSource，可直接 pub use（屬正式 PL 出口，不算 transition）。
  layering_rule: |
    service 層內檔案不得 import rusqlite 或 crate::*::dao::*。
    BookSource + Rule DSL 屬 service 層，純資料 + 純解析邏輯，無 SQL，符合此約束。

Test:
  unit_tests_to_preserve:
    - 4 個 rule DSL #[test]：parse_basic / parse_attr_and_replace / parse_alternatives / extract_text_with_fallback
    - 搬移後 path 變為 catalog::service::rule::tests
    - 驗證指令：`cargo test catalog::service::rule::tests` 必過
  regression_checks:
    - cargo check 退出碼 0
    - cargo build 退出碼 0；warning 數量 ≤ 基線 2 條
    - 無 unused_imports / no_use 痕跡
  structural_checks:
    - `grep -rn "use rusqlite" src/catalog/service/` 無輸出
    - `grep -rnE "use crate::[a-z]+::dao" src/catalog/service/` 無輸出
    - `grep -rn "use crate::source::" src/` 0 matches（全部已改為 crate::catalog::*）
    - find src -type d 不再出現 src/source

Constraints:
  env:
    - LIBCLANG_PATH=/usr/lib/llvm-18/lib 為所有 cargo 指令必要前綴；wreq → boring-sys2 → bindgen 需要 libclang
  git:
    - branch: refactor/ddd-context-split
    - 禁止 commit .claude/{wip,analyze,skills}/
    - 不在 main 分支直接 refactor
  build_discipline:
    - 禁止 `cargo clean`（首次編譯 ~2-3 分鐘）
    - 每個 task 結束必跑 `cargo build` 確認可編譯
  scope_boundaries:
    - .claude/skills/ 不可修改（REQ-006）
    - src/library/* 不再被本 task 結構性改動，只可更新 import 路徑（如 src/library/dao.rs 對 BookSource 的 use）
    - scraper.rs / dao.rs / cli.rs / reader.rs / backup.rs：本 task 僅做 import path 更新，不搬內容
    - SQLite schema 不變（不在本 task 範圍）
    - CLI grammar 不變（不在本 task 範圍）
  warning_budget:
    - 重構後 warning ≤ 2（基線：select_within + extract_all_doc dead_code）
    - 既有 dead_code 警告（select_within, extract_all_doc）為設計保留，搬移後仍會出現屬正常
  module_visibility:
    - BookSource 必須 pub re-export 在 src/catalog/mod.rs：`pub use service::source::BookSource;`
    - 確保 cli.rs / reader.rs / backup.rs / library/dao.rs 可透過 `crate::catalog::BookSource` 引用
  test_must_pass:
    - cargo test catalog::service::rule::tests 4 個 case 必過

Files:
  create:
    - path: src/catalog/mod.rs
      content_hint: |
        //! Catalog: 描述如何從網站抽資料並執行抽取
        //! Outbound PL: SearchHit / Novel / Vec<ChapterMeta> / RawContent
        pub mod service;
        pub mod dao;     // 後續 task TASK-catalog-03 填入
        pub mod facade;  // 後續 task TASK-catalog-03 填入
        pub use service::source::BookSource;
    - path: src/catalog/service/mod.rs
      content_hint: |
        pub mod source;
        pub mod rule;
        pub mod scraper;  // 後續 task TASK-catalog-02 搬入
    - path: src/catalog/service/source.rs
      content_hint: 從 src/source/mod.rs 整檔搬入，移除 `pub mod rule;` 那一行
    - path: src/catalog/service/rule.rs
      content_hint: 從 src/source/rule.rs 整檔搬入（含 #[cfg(test)] mod tests 4 case）
  modify:
    - path: src/main.rs
      change: 移除 `mod source;`，加 `mod catalog;`
    - path: src/scraper.rs
      change: import path 更新（use crate::source::{rule, BookSource} → crate::catalog::service::{rule, source::BookSource} 或 crate::catalog::BookSource）
    - path: src/library/dao.rs
      change: import path 更新，BookSource 改為 crate::catalog::BookSource
    - path: src/cli.rs
      change: import path 更新，BookSource 改為 crate::catalog::BookSource
    - path: src/reader.rs
      change: import path 更新，BookSource 改為 crate::catalog::BookSource
    - path: src/backup.rs
      change: import path 更新，BookSource 改為 crate::catalog::BookSource
  delete:
    - src/source/mod.rs
    - src/source/rule.rs
    - src/source/ (directory)
  do_not_touch:
    - .claude/skills/
    - .claude/analyze/
    - .claude/wip/
    - src/library/service/
    - src/library/facade.rs
    - src/library/mod.rs
    - Cargo.toml / Cargo.lock（無 dependency 變動）
    - book-sources/*.json
    - src/storage.rs（schema 不變；本 task 與其無關）
    - src/config.rs
    - src/models.rs（SearchHit 留待 TASK-catalog-02 才搬）

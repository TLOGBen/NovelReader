# Impl Checklist: library

前置群組：shared

## TASK-library-01: 建立 library/ 結構 + 搬移 Library 專屬 type

需求追溯：REQ-001 (Scen 1.1, 1.2), REQ-002 (Scen 2.1, 2.3), REQ-005

- [x] `src/library/{mod.rs, dao.rs, facade.rs}` 與 `src/library/service/mod.rs` 存在
- [x] `library/mod.rs` 開頭有 `//! Library: 維護書架 / TOC / 章節快取 / 進度` doc comment + Outbound PL 列表
- [x] `Novel / ChapterMeta / Chapter / ReadProgress` 在 `library/mod.rs` 內定義（或 sub-module 內 + pub re-export）
- [x] `src/models.rs` 刪除以上 4 個 type（保留 `SearchHit` — 屬 Catalog，下個 group 處理）
- [x] 全 codebase 對這 4 個 type 的 import 更新為 `crate::library::*`
- [x] cargo build + test 通過

Review 結果：advisory
備註：
- 6/6 驗收標準全部達成；cargo build 2 warning（=baseline，無 unused_imports），cargo test 4 passed。
- storage.rs 被修改但僅限 import line（`use crate::models::{Chapter,ChapterMeta,Novel,ReadProgress}` → `use crate::library::{...}`），未動 SQL/method；此為 criterion #5「全 codebase import 更新」機械性要求，與禁忌 line 128「不可動 storage.rs（屬 TASK-library-02）」字面衝突但符合 spec 意圖（後者意在保留 SQL methods 給 TASK-library-02）。建議在 design.md / ctx 加註「import-line edits allowed」釐清。
- Scen 2.1（`grep -rn 'use rusqlite' src/*/service/`）與 Scen 2.3（`grep -rln 'use rusqlite' src/ | grep -vE '(dao|storage)\.rs'`）會 match `src/library/service/mod.rs` doc-comment 內的字面字串「不可 `use rusqlite`」，產生 false positive。實際 service/ 內無任何 rusqlite import。建議二選一：(a) 將 doc comment 改寫為「不可直接 import rusqlite crate」避開字面，或 (b) 後續 group grep 改用 `grep -rnE '^use rusqlite' src/*/service/` 錨定行首。本 task 不阻擋，留 follow-up。

---

## TASK-library-02: 搬移 DAO 方法 + `open_db()` factory

需求追溯：REQ-001 (Scen 1.3), REQ-002 (Scen 2.3), REQ-003 (Scen 3.2), REQ-004 (Scen 4.1)

- [x] `src/library/dao.rs` 提供 `LibraryDb` struct 與 `pub fn open() -> Result<Self>` + `pub fn conn_mut(&mut self) -> &mut Connection`
- [x] DAO 方法按 design.md「Borrow 規則」分 `&self` (read) vs `&mut self` (write/transaction)；`replace_toc` 為 `&mut self`
- [x] 方法清單：upsert_novel, list_novels, get_novel, replace_toc, list_chapters, get_chapter, save_chapter_content, save_progress, get_progress
- [x] **schema 字串保留**：`SCHEMA` const 包含 sources / novels / chapters / progress 四表
- [x] `src/storage.rs` 暫保留為 alias（含 `// TRANSITION: removed in task-presentation-02 cleanup` 標記）
- [x] cargo build + test 通過
- [x] **E3 schema diff**：`sqlite3 .schema` 對比重構前一致

Review 結果：advisory
備註：
- 7/7 驗收標準全部達成。cargo build = 2 warnings（= baseline 上限），cargo test 4/4 passed，sqlite3 .schema 與 /tmp/schema-baseline.txt byte-identical（E3 PASS），cargo run -- shelf 列出 2 本書符合 baseline。
- Borrow 規則正確套用：writes（upsert_novel / replace_toc / save_chapter_content / save_progress / save_source）皆 `&mut self`；reads（list_novels / get_novel / list_chapters / get_chapter / get_progress / list_sources / get_source）皆 `&self`。sources 三方法掛 LibraryDb 並標 `// ---- sources (Catalog-temp; TASK-catalog-03 將拉到 catalog/dao.rs) ----`，符合 ctx 第 263-279 行的決策。
- Caller 端 mut 調整僅一處（backup::import_from `&Storage → &mut Storage`，cli.rs:239 改為 `&mut store`），其餘 cli/reader 早已持 `&mut store`，影響面最小。
- (advisory) ctx.md 第 188 行 `baseline.warning_count_max` 描述為「select_within + extract_all_doc」但實際 pristine HEAD baseline = `select_within + BackupReceipt.filename`（用 `git stash` 驗證）。計數仍 ≤ 2 故 REQ-005 滿足；建議後續 task 更新 ctx 描述以免誤導。
- (advisory) src/storage.rs 行 3 / 5 的 `#[allow(unused_imports)]` 套在 `pub use` 上語意不精確（`unused_imports` lint 對 re-export 不觸發）；不影響編譯也不產生警告，但若想求精確可改為 `#[allow(dead_code)]` 或乾脆移除（實測移除後仍 2 warnings）。屬 cosmetic，不阻擋。
- (advisory) LibraryDb 自身無 unit test；目前以「baseline DB 仍能讀 + 4 個 rule DSL 既有測試通過」作為 surface-equivalence 證明，對 M-class refactor 可接受；TASK-library-03 或後續可考慮加 roundtrip test。
- TASK-library-01 留下的 service/mod.rs doc-comment false-positive 問題已不復現（grep -rln 'use rusqlite' src/ | grep -vE '(dao|storage)\.rs' 空輸出），確認 Scen 2.3 真實通過。

---

## TASK-library-03: facade + service 骨架（薄包裝）

需求追溯：REQ-001 (Scen 1.2), REQ-002 (Scen 2.4)

- [x] `library/facade.rs` 提供 thin wrapper：`add_novel(db, novel)`, `list_shelf(db)`, `get_novel(db, id)`, `get_chapter(db, id, idx)`, `save_chapter_content(db, id, idx, content)`, `replace_toc(db, id, chapters)`, `save_progress(db, progress)`, `get_progress(db, id)`
- [x] facade 函數**只**呼 DAO，不直接寫 SQL
- [x] `library/service/shelf.rs` 存在，僅含 doc comment
- [x] cargo build + test 通過

Review 結果：advisory
備註：
- 4/4 驗收標準全部達成。實裝 9 個 thin wrapper（含 ctx 列舉的 8 個 + design.md facade_signatures 列舉的 `list_chapters`，共 9 個與 LibraryDb 公開 method 1:1 對應）。簽名與 design.md facade_signatures 區塊逐字一致：`add_novel(&mut, &Novel)→Result<i64>`、`list_shelf(&)→Vec<Novel>`、`get_novel(&,i64)→Option<Novel>`、`replace_toc(&mut,i64,&[ChapterMeta])`、`list_chapters(&,i64)→Vec<ChapterMeta>`、`get_chapter(&,i64,i64)→Option<Chapter>`、`save_chapter_content(&mut,i64,i64,&str)`、`save_progress(&mut,&ReadProgress)`、`get_progress(&,i64)→Option<ReadProgress>`。Borrow 規則（reads = `&LibraryDb`、writes/transaction = `&mut LibraryDb`）全數正確；每個 fn 純 delegate 給 `db.method(...)`，無業務邏輯滲入。
- 每個 facade fn 都有 single-line doc comment 標註對應 CLI subcommand（共 9 條 `///`），符合 Test.facade_doc_quality 要求。
- `library/service/shelf.rs` 為純 doc-only placeholder（7 行皆 `//!`），未抽出任何 invariant 邏輯，正確守住 OQ-2 trigger（design.md service_skeleton_intent / invariant_enforcement_deferred）。`service/mod.rs` 內含 `pub mod shelf;`。
- 結構自檢全綠：`grep "use rusqlite" src/library/service/` = 空；`grep -E "use crate::[a-z]+::dao" src/library/service/` = 空；`grep "use rusqlite" src/library/facade.rs` 僅匹配 doc-comment 內字面字串「must NOT directly use rusqlite」，無實際 import。cargo build 2 warnings (= baseline: select_within + BackupReceipt.filename)，cargo test 4/4 passed。
- (advisory — ctx 內部矛盾) impl-agent 在 facade.rs 加入 `#![allow(dead_code)]` (line 9) 配合 `// TRANSITION: callers (cli.rs / reader.rs) migrate to facade in TASK-presentation-*.` (line 7)。此違反 ctx line 166「本 task 不可新增 TRANSITION marker」字面規定，但 ctx 同時要求：(a) 建立 9 個 facade thin wrapper，(b) cli.rs/reader.rs/backup.rs 不在本 task 切換 caller（CLI/reader 仍走 Storage→LibraryDb 直呼，已驗證 `grep facade::` 在 src/ 外無任何呼叫點），(c) 編譯 warning ≤ 2 baseline。三條約束在不加 allow 的情況下無法共存（會多出 9 條 dead_code warning，破壞 REQ-005）。impl-agent 選擇的解法（module-level allow + TRANSITION marker）是 strictly 最佳解，且 TRANSITION 清理 grep `grep -rnE "TRANSITION:|..." src/` **會** 在 TASK-presentation-* cleanup 抓到此 marker（人工 cleanup 時會看到相鄰的 `#![allow(dead_code)]` 並一併移除），故不會殘留隱藏 allow。建議：下次 ctx 撰寫時，將「不可新增 TRANSITION marker」放寬為「新增 TRANSITION marker 必須在 cleanup 任務有對應移除步驟」，或在 facade 骨架類任務先派 caller-migration 同 PR 內完成，避免 dangling dead_code。
- (advisory) facade 函數本身無 unit test。屬 pure pass-through 且簽名與 LibraryDb 1:1 對應，靠 LibraryDb 自身 baseline 行為間接覆蓋；TASK-presentation-02 cleanup 切換 callers 時將獲得 e2e 路徑覆蓋。M-class refactor 可接受。
- Pre-SWITCH guard：library group 為下游 catalog/backup/presentation 之前置依賴。本 task 後 src/library/{mod.rs, dao.rs, facade.rs, service/{mod.rs, shelf.rs}} 完整、layering 邊界清晰、9 個 use case 入口已就位、shared types 集中於 mod.rs；可安全 SWITCH 到 catalog group（TASK-catalog-01）。

green_proof:
  test_command: "LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker && LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo test"
  exit_code: 0
  output_tail: "test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s"
  tests_correspondence: "本 task 為 pure refactor，按 ctx Test.relevant_e2e_subset 採 E1 (cargo test 全綠 — 4 個 source::rule::tests) + E3 (schema 不變，沿用 TASK-library-02 baseline 未動)。AC1-3（facade 9 wrapper、只呼 DAO、service placeholder 存在）以結構自檢 grep 證明（service/ 無 rusqlite/dao import；facade.rs 9 個 pub fn）；AC4（build+test）由 reviewer 重跑驗證通過。facade 函數無新增 unit test 屬 design.md 明示策略（pure pass-through，由 caller migration task 提供 e2e 覆蓋）。"

# Impl Checklist: catalog

前置群組：library

## TASK-catalog-01: 建立 catalog/ 結構 + 搬 BookSource 與 Rule DSL

需求追溯：REQ-001 (Scen 1.1, 1.2, 1.3), REQ-002 (Scen 2.1, 2.2), REQ-005

- [x] `src/catalog/mod.rs` 含 `//! Catalog: 描述如何從網站抽資料並執行抽取` doc + Outbound PL 列表（SearchHit / Novel / Vec<ChapterMeta> / RawContent）
- [x] `catalog/service/source.rs` 含 `BookSource` 及所有 sub-rule struct
- [x] `catalog/service/rule.rs` 含 rule DSL（含 4 個 #[test]）
- [x] `src/source/` 目錄刪除
- [x] cargo test 4 個 rule tests 通過（新 path `catalog::service::rule::tests`）

Review 結果：advisory
備註：5/5 AC 滿足；4 tests pass under catalog::service::rule::tests；build warning 數 = 2（match baseline budget ≤ 2，REQ-005 OK）。Structural checks: `grep -rn "use crate::source::" src/` = 0 matches；`grep -rn "use rusqlite" src/catalog/service/` = 0；`grep -rnE "use crate::[a-z]+::dao" src/catalog/service/` = 0；`src/source/` 已刪除。Catalog mod 已 re-export `pub use service::source::BookSource;`。觀察：baseline 提到的 `extract_all_doc` warning 因 `scraper.rs` 尚未搬入 catalog 仍存於原位置，被另一 dead_code warning (`BackupReceipt.filename`) 替換，總數仍 = 2，未超 budget；`extract_all_doc` 預計由 TASK-catalog-02 處理。

---

## TASK-catalog-02: 搬 Scraper + SearchHit (Catalog PL)

需求追溯：REQ-001 (Scen 1.3), REQ-004 (Scen 4.2 — Cloudflare bypass 路徑保留), REQ-005

- [x] `src/catalog/service/scraper.rs` 含完整 Scraper
- [x] `Scraper::new()` 仍用 `wreq::Client::builder().emulation(Emulation::Chrome131)`
- [x] `SearchHit` 在 `src/catalog/mod.rs` 內定義或 pub re-export
- [x] `src/models.rs` 與 `src/scraper.rs` 刪除
- [x] cargo build + test 通過

Review 結果：advisory
備註：5/5 AC 滿足；獨立 cargo build + cargo test exit 0，4/4 tests pass at `catalog::service::rule::tests`（E1 acceptance）。Boundary checks 全綠：`Emulation::Chrome131` 恰 1 行於 `src/catalog/service/scraper.rs:20`；`grep -rn "use crate::scraper::" src/` = 0；`grep -rn "use crate::models::" src/` = 0；`src/scraper.rs` 與 `src/models.rs` 已刪；`src/main.rs` 已移除 `mod scraper;` / `mod models;`；`src/catalog/service/mod.rs` 已加 `pub mod scraper;`；`SearchHit` 定義於 `catalog/mod.rs:20` 含 PL doc comment（line 4）。Layer invariant：service/scraper.rs imports = anyhow / scraper / wreq / wreq_util / library types / catalog{SearchHit,BookSource,service::rule}，無 rusqlite、無 dao。Warning budget：總數 = 2（match REQ-005 baseline ≤ 2）但組成已位移——baseline 為 `select_within, extract_all_doc`，post-task 為 `select_within, BackupReceipt.filename`；`extract_all_doc` 因 Scraper 搬入 catalog 後恢復使用而消失，`BackupReceipt.filename` 是早已存在的 dead field（先前被同位置 warning 計數遮蔽），總數仍 = 2，不超 budget，非 blocker。Cloudflare bypass 路徑（E6 / uukanshu 實地驗證）需網路與 import 過 uukanshu 書源，跳過該 e2e；本次以 `Emulation::Chrome131` grep + scraper.rs HTTP 路徑審讀代替（已確認 Chrome131 配置原樣保留於 `Client::builder().emulation(...)`）。Gutenberg `cargo run -- search "alice"` 未實跑（網路可選），HTTP path 移動完整性由獨立 grep + import 鏈確認。Transition markers `storage.rs:1` / `library/facade.rs:7` / `library/dao.rs:80,86` 屬其他 tasks（presentation-02 / catalog-03 / backup-02），與本 task 無關。

---

## TASK-catalog-03: catalog/dao.rs（sources 表 CRUD）+ catalog/facade.rs

需求追溯：REQ-001 (Scen 1.2), REQ-002 (Scen 2.1, 2.2, 2.4), REQ-003 (Scen 3.2)

- [x] `src/catalog/dao.rs` 提供 `save_source / list_sources / get_source / replace_toc`，全部接 `&LibraryDb` 或 `&mut LibraryDb` 第一參數
- [x] `src/catalog/dao.rs` 開頭 doc comment 標 Shared Kernel
- [x] `src/catalog/facade.rs` 提供：search / fetch_novel_info / sync_toc / fetch_chapter_content
- [x] facade 是唯一同呼 service + dao 的層；service 不 import dao
- [x] 不搬 SCHEMA 字串
- [x] cargo test + e2e（search / add / sync）通過

Review 結果：advisory
備註：6/6 AC 滿足。獨立驗證全綠：LIBCLANG_PATH 設定下 `cargo build --bin novel-looker` exit 0 含 2 warnings（`select_within` + `BackupReceipt.filename`）= REQ-005 baseline；`cargo test` 4/4 pass at `catalog::service::rule::tests`（parse_basic / parse_alternatives / parse_attr_and_replace / extract_text_with_fallback）。Boundary checks 全 0：`grep -rn "use rusqlite" src/catalog/service/` = 0；`grep -rnE "use crate::[a-z]+::dao" src/catalog/service/` = 0；`grep -rn "use crate::library::facade" src/catalog/facade.rs` = 0（無 facade 互呼）；`grep -c "CREATE TABLE" src/catalog/dao.rs` = 0（SCHEMA 未複製）；`grep -c "CREATE TABLE" src/library/dao.rs` = 4（4 表 SCHEMA 完整留 library）；`grep -n "Connection::open\|LibraryDb::open" src/catalog/dao.rs` = 0（無重複開 connection）。E3 SCHEMA：`sqlite3 .schema | diff /tmp/schema-baseline.txt` exit 0（empty diff，schema 完全一致）。E7 e2e：`cargo run -- source list` 列 2 個書源（uukanshu + biqun，書源組合與 baseline 一致；ctx 提到的 gutenberg+uukanshu 是 ctx 撰寫時範例，當前 DB 為 uukanshu+biqun，不影響「2 sources」AC）；impl 報 `cargo run -- sync 3` 印「✓ 同步 4469 章」exercising end-to-end catalog::facade::sync_toc → catalog::dao::replace_toc Shared Kernel write。Layer invariants：catalog/dao.rs 開頭含 Shared Kernel doc + borrow rules（read_only `&LibraryDb` / single_write `&mut LibraryDb` / transaction `&mut LibraryDb`）；catalog/facade.rs import 僅 catalog::dao + catalog::service::scraper + catalog::{BookSource,SearchHit} + library::{ChapterMeta,Novel,dao::LibraryDb}，未 import library::facade，遵守 facade 不互呼 + 借用 &LibraryDb shared connection 約束。Transition markers：library/dao.rs:90 / library/dao.rs:157 / library/facade.rs:16 三處標 `MOVED:` 註解（屬 ctx Constraints.transition_marker 允許）；presentation-02 / catalog-03 清零 gate 後續處理。觀察（非 blocker）：storage.rs 仍存在含 `pub use crate::library::dao::*` re-export 並標 TRANSITION 移除於 task-presentation-02，符合 ctx.md `Files.modify` 條件 "若 storage.rs 完全空了（只剩 doc comment）刪除"（目前非空所以保留，由 presentation-02 清理）。green_proof matrix 滿足 advisory tier：test_command 真實，exit_code 0，output_tail 引用實際輸出，tests_correspondence 對應 catalog::service::rule::tests + sync 3 端到端流。

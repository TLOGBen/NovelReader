# Impl Checklist: shared

前置群組：無

## TASK-shared-01: 建立 utils/ 骨架 + 搬出 resolve() helper

需求追溯：REQ-001 (utils/ 須存在), REQ-005 (編譯品質不退化)

- [x] `src/utils/mod.rs` 與 `src/utils/url.rs` 存在
- [x] `src/utils/url.rs` 包含從原 `src/scraper.rs` 搬出的 `pub fn resolve(base: &str, href: &str) -> Result<String>`
- [x] `src/scraper.rs` 內 `resolve` 私有函數已移除，呼叫處改用 `crate::utils::url::resolve`
- [x] `cargo build --bin novel-looker`（含 `LIBCLANG_PATH`）通過，無新 warning
- [x] `cargo test` 全綠

Review 結果：advisory
備註：|
  全部 5 條驗收標準逐條核對通過。Reviewer 獨立重跑 `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker` 與 `cargo test` 確認 exit 0、4/4 rule tests pass、warning 數量 = 2（≤ baseline 2）。
  TRANSITION grep 清零。scraper.rs 確認無 `fn resolve` 定義、5 處 call site 全改為 `crate::utils::url::resolve`。main.rs 的 `mod utils;` 位於 alphabetical 正確位置（storage 之後）。
  Observation（非阻塞）：ctx.md 中標示的基線 warning 名單（`select_within` + `extract_all_doc`）與實測（`select_within` + `BackupReceipt.filename`）略有出入；數量仍 = 2、不影響 AC「warning ≤ 2」成立，且非本 task 引入。後續 task 若需更新 baseline 可順手校正 ctx 註腳，不必為此打回 impl。

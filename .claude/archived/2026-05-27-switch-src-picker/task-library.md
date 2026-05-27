# Tasks: library
**前置群組**：無

> library::facade::switch_source_tx 加 `target_idx: Option<i64>`；dao step 4 改 `target_idx.unwrap_or(first_idx)`；既有 fault-injection UT cascade。可與 shared 並行（不同檔）。

---

## TASK-library-01: switch_source_tx 簽名擴 + dao step 4 改 + UT cascade

**需求追溯**：REQ-003 / REQ-006
**目標**：library::facade::switch_source_tx 與底層 dao update_book_source_tx_inner 加 target_idx: Option<i64>；任一 caller 給 Some(N) 就寫 progress.chapter_index = N、給 None fall back first_idx；既有 atomicity / fault-injection 行為不退化；defensive：target_idx 越界視為 None。

**驗收標準**：
- [ ] library/facade.rs:53 簽名 `pub fn switch_source_tx(db, novel_id, new_src_url, new_book_url, new_chapters, target_idx: Option<i64>) -> Result<i64>`
- [ ] dao update_book_source_tx_inner step 4 改 `target_idx.unwrap_or(first_idx)` 
- [ ] defensive：target_idx=Some(N) 但 `N < 0 || N >= new_chapters.len() as i64` → fall back first_idx、不 panic
- [ ] facade 回 `Result<i64>` 寫入的 progress.chapter_index 實際值（caller 可用此值寫 SwitchOutcome.new_progress_idx）
- [ ] 既有 5 個 dao fault-injection UT call site 加 None 參數、行為不退化
- [ ] UT INT-switch-target-some-01 + INT-switch-target-none-02 + INT-switch-target-out-of-bounds-01 全綠

### 步驟

#### 1. facade 簽名擴
- [ ] Read library/facade.rs:50-65 看 switch_source_tx 既有簽名 + doc comment
- [ ] 改簽名加 `target_idx: Option<i64>` 參數
- [ ] forward 給 dao 並回 dao 寫入值

#### 2. dao step 4 改
- [ ] Read library/dao.rs:279-371 看 update_book_source_tx_inner 完整邏輯
- [ ] step 4（line ~320-365 附近）UPSERT progress 處改：
  - 算 `let resolved_idx = target_idx.and_then(|i| if i >= 0 && (i as usize) < new_chapters.len() { Some(i) } else { None }).unwrap_or(first_idx)` 
  - SQL params 用 resolved_idx 取代既有 first_idx hardcoded
- [ ] fn 回 resolved_idx
- [ ] 內部 `first_idx = new_chapters.first().unwrap().index` 邏輯不動（仍是 None 時 fall back 依據）

#### 3. 既有 fault-injection UT cascade
- [ ] grep `switch_source_tx(` 找所有 caller、include test
- [ ] dao 既有 fault-injection UT（5 個 abort case + 1 happy path）call site 加 None 參數
- [ ] 跑 `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo test library::dao::tests` 確認既有全綠

#### 4. UT
- [ ] INT-switch-target-some-01：
  - 建 in-memory DB、seed novels + sources、mock new_chapters 5 章 (index 0,1,2,3,4)
  - 呼 switch_source_tx(.., target_idx=Some(3))
  - 驗 回值 = 3
  - SELECT progress WHERE novel_id → chapter_index = 3, scroll = 0
- [ ] INT-switch-target-none-02：同 setup、target_idx=None → 回值 = 0（first_idx）、SELECT 驗 chapter_index = 0
- [ ] INT-switch-target-out-of-bounds-01：target_idx=Some(99)（new_chapters 5 章、99 上越界）→ 回值 = first_idx = 0、不 panic
- [ ] INT-switch-target-out-of-bounds-02：target_idx=Some(-1)（負數）→ 回值 = first_idx = 0、不 panic；progress.chapter_index 寫 0
- [ ] 兩條對應 test.md REQ-003 表的 INT-switch-target-out-of-bounds-01/02

#### 5. baseline 驗證
- [ ] `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo test library 2>&1 | tail -15` 全綠
- [ ] cargo build 無新 warning

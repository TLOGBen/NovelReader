# Tasks: handler-core
**前置群組**：library

> switch_source_core::run + SwitchSourceDeps trait method 簽名擴；SwitchOutcome 加 new_progress_chapter_name；CLI handler 傳 None 兼容；既有 5 fault-injection UT cascade。

---

## TASK-handler-core-01: switch_source_core::run + SwitchSourceDeps + SwitchOutcome 擴 + CLI 兼容

**需求追溯**：REQ-003 / REQ-006
**目標**：switch_source_core 三層（pub fn run + trait method + impl）簽名同步加 target_idx；SwitchOutcome 加 new_progress_chapter_name 由 run 內 `new_toc[target_idx as usize].name` 取；CLI handler 傳 None；既有 5 fault-injection UT 不退化。

**驗收標準**：
- [ ] switch_source_core.rs:141 `pub async fn run(ctx, novel_id, new_src_url, new_book_url, target_idx: Option<i64>)` 簽名擴
- [ ] SwitchSourceDeps trait method `switch_source_tx` 簽名加 target_idx: Option<i64>
- [ ] RealDeps impl forward 給 facade（含新 target_idx 參數）
- [ ] SwitchOutcome 加 `pub new_progress_chapter_name: String` 欄位
- [ ] run 內 SwitchOutcome 組裝：`new_progress_chapter_name = new_toc[written_idx as usize].name.clone()`（用 facade 回值索引 toc）
- [ ] 既有 5 個 fault-injection UT call site 加 None 參數、全綠不退化
- [ ] CLI handler `presentation/handlers/switch_source.rs` call site 加 None 參數、output 訊息與本次擴前等價
- [ ] UT INT-switch-outcome-name-01 + INT-switch-outcome-name-02 + INT-switch-deps-trait-01 + INT-switch-fault-injection cascade 全綠

### 步驟

#### 1. trait 簽名擴
- [ ] Read switch_source_core.rs:77-94 看 SwitchSourceDeps trait 既有定義
- [ ] trait method `switch_source_tx` 加 target_idx: Option<i64>
- [ ] Read switch_source_core.rs:117-132 看 RealDeps impl 既有 forward
- [ ] RealDeps::switch_source_tx 加 target_idx 並 forward 給 library::facade::switch_source_tx

#### 2. SwitchOutcome 擴
- [ ] Read switch_source_core.rs:67-71 看 SwitchOutcome 既有 3 欄位
- [ ] 加 `pub new_progress_chapter_name: String`
- [ ] 既有 `new_first_chapter_name` 保留（CLI target_idx=None 路徑仍用）

#### 3. run 簽名擴 + outcome 組裝
- [ ] Read switch_source_core.rs:141-206 看 run 既有 5 step
- [ ] run 簽名加 target_idx: Option<i64>
- [ ] step 5 後（facade 回 written_idx 之後）組裝：
  - `let new_progress_chapter_name = new_toc.get(written_idx as usize).map(|c| c.name.clone()).unwrap_or_default();`
  - SwitchOutcome { new_progress_idx: written_idx, new_first_chapter_name: new_toc.first().unwrap().name.clone(), new_progress_chapter_name, chapter_count: new_toc.len() as i64 }
- [ ] forward target_idx 給 deps.switch_source_tx

#### 4. 既有 5 fault-injection UT cascade
- [ ] grep `run(` 與 `switch_source_tx(` 在 switch_source_core.rs tests mod 內所有 caller
- [ ] 5 個 abort case UT call site 加 None
- [ ] 既有 mock SwitchSourceDeps impl 也加 target_idx 參數（mock 內 ignore 或 record）
- [ ] 跑 `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo test switch_source_core` 全綠

#### 5. CLI handler 兼容
- [ ] Read src/presentation/handlers/switch_source.rs 看 CLI handler 呼 run 處
- [ ] call site 加 None 參數
- [ ] 確認 output 訊息使用 `outcome.new_first_chapter_name` 或 `outcome.new_progress_chapter_name`（CLI target=None 兩者皆 = 第一章名、選哪個都正確）
- [ ] cargo build 無新 warning

#### 6. 新 UT
- [ ] INT-switch-outcome-name-01：mock new_toc 5 章 [name="序章", "第 1 章", ..., "第 4 章"]、呼 run with target_idx=Some(3) → SwitchOutcome.new_progress_chapter_name == "第 3 章"
- [ ] INT-switch-outcome-name-02：同 setup target_idx=None → new_progress_chapter_name == "序章"（first.name）
- [ ] INT-switch-deps-trait-01：mock SwitchSourceDeps impl 收 target_idx 並 record；呼 run with Some(2) → mock 收到 Some(2)
- [ ] INT-switch-fault-injection cascade：既有 5 abort case 重跑 with None

#### 7. baseline 驗證
- [ ] `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo test 2>&1 | tail -15` 全綠
- [ ] cargo build 無新 warning

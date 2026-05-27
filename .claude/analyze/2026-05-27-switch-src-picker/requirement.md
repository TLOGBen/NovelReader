# Requirements

## REQ-001: SearchPickerScreen 並行 streaming UX

**描述**：按 s 觸發後即時跳出 picker、所有 enabled 書源並行打 search、每書源回來就 append；timeout 5s 不阻塞 picker UI；使用者可在任何時候 Enter 已有結果。

### Scenarios

**Scenario 1：picker 開啟**
- **Given** 使用者在 shelf 或 reader 內
- **When** 按 `s` 鍵
- **Then** SearchPickerScreen 立即顯示 Picking phase
- **And** 表頭顯示「搜尋: <書名> / <作者>」（書名與作者從當前 highlight novel 或 reader.novel_id 取）
- **And** picker 表格初始空白

**Scenario 2：各書源結果 streaming append**
- **Given** picker Picking phase 已開啟、N enabled 書源 search 並行進行
- **When** 任一書源 search 回來
- **Then** picker 表格 append 一行（書源 URL × 書名 × 作者 × 章數 × 狀態欄）
- **And** 表格不阻塞 UI（其他書源 search 持續跑）

**Scenario 3：timeout 書源不阻塞**
- **Given** 某書源 search 進行中
- **When** 該書源耗時超過 5 秒
- **Then** 該書源行顯示「逾時 (5s)」灰色字
- **And** 其他書源結果仍正常顯示
- **And** picker UI 不卡頓

**Scenario 4：Enter 提早選**
- **Given** picker 列表至少有 1 行成功狀態的結果
- **When** 使用者按 Enter
- **Then** `JoinSet::abort_all()` 立即執行、剩餘 pending search 結果丟棄、不再 append
- **And** SearchPickerScreen 切換到 Confirming phase
- **And** 選中行成為 Confirming 對象

**Scenario 5：Esc 取消**
- **Given** picker 任何 phase
- **When** 使用者按 Esc
- **Then** picker 關閉、不寫 DB
- **And** transition 回原 caller screen（依 entry 決定，見 REQ-004 S3）

---

## REQ-002: Fuzzy 章節 mapping with anchor

**描述**：Confirming phase 顯示舊→新章節對應 + score；score > 50 接受 confirm、否則 abort；多筆同分用 `|new_idx as i64 - anchor| asc` tiebreak；異常邊界明確處理。

### Scenarios

**Scenario 1：高分對應接受 confirm**
- **Given** Confirming phase 對選中書源、`apply_fuzzy_filter(舊章名, 新源 toc, Some(舊 idx as i64))` 回 best (new_idx, score) 且 score > 50
- **When** 顯示「舊源第 X 章 《名》 → 新源第 Y 章 《名》(score N)」
- **And** 使用者按 `y`
- **Then** 進入後端 atomic 換源（呼 `switch_source_core::run(..., target_idx: Some(new_idx as i64))`）

**Scenario 2：低分 abort 不寫 DB**
- **Given** Confirming phase、best score ≤ 50
- **When** picker 渲染對應行
- **Then** 顯示「找不到對應章節 (best score N ≤ 50)」
- **And** 不接受 `y`
- **And** 按 Esc 回 Picking phase、不寫 DB

**Scenario 3：同分 tiebreak**
- **Given** apply_fuzzy_filter 對新 toc 跑分後有多筆同 best score
- **When** Confirming phase 取「最佳對應」
- **Then** 取 `|new_idx as i64 - anchor| asc` 最小者；若 anchor=None 走 stable order 第一筆

**Scenario 4：新源 toc==0**
- **Given** 選中書源 confirm 後 `switch_source_core::run` 走 sync_toc → 新 toc 為空
- **When** run 內 `evaluate_toc` 回 `AbortReason::EmptyToc`
- **Then** Confirming phase 顯「新源無章節 (best score N/A)」
- **And** Esc 回 Picking、不寫 DB

**Scenario 5：舊章查不到 chapters table**
- **Given** 進 picker 前 shelf 或 reader 嘗試取 `library::facade::get_chapter(novel_id, current_idx)`
- **When** chapter row 不存在（資料壞 / progress idx 越界）
- **Then** SearchPickerScreen 不開啟、回原 caller 帶 toast「找不到舊章節，無法換源」
- **And** anchor 路徑邏輯不會走到 fuzzy（前置 abort）

**Scenario 6：apply_fuzzy_filter 接 anchor=None**
- **Given** apply_fuzzy_filter caller 不提供 anchor（例如 reader Filter mode 既有兩 caller）
- **When** 計算 ranking
- **Then** 只用 primary `score desc`、不加 secondary key
- **And** 結果排序與本次擴前等價

---

## REQ-003: Atomic 換源 contract 擴

**描述**：`library::facade::switch_source_tx` / `switch_source_core::run` / `SwitchSourceDeps::switch_source_tx` 三層簽名加 `target_idx: Option<i64>`；`SwitchOutcome` 新加 `new_progress_chapter_name: String`；progress.chapter_index 寫入值 = target_idx 給定值或 fall back first_idx；任一階段失敗整 tx rollback。

### Scenarios

**Scenario 1：target_idx=Some(N) 寫入路徑**
- **Given** picker confirm 觸發 `switch_source_core::run(ctx, novel_id, new_src_url, new_book_url, target_idx: Some(N))`
- **When** run 完整成功
- **Then** `library::facade::switch_source_tx` 收 `target_idx: Some(N)`、`dao step 4` 寫入 `progress.chapter_index = N`
- **And** `SwitchOutcome.new_progress_idx = N`
- **And** `SwitchOutcome.new_progress_chapter_name = new_toc[N as usize].name`

**Scenario 2：target_idx=None 寫入路徑（CLI 兼容）**
- **Given** CLI `switch-source` 子命令呼 `run(..., target_idx: None)`
- **When** run 完整成功
- **Then** `switch_source_tx` 收 `target_idx: None`、`dao step 4` fall back `first_idx = new_toc.first().unwrap().index`
- **And** `SwitchOutcome.new_progress_idx = first_idx`
- **And** `SwitchOutcome.new_progress_chapter_name = new_toc[0].name`
- **And** 行為與本次擴前等價

**Scenario 3：tx rollback**
- **Given** switch_source_tx 內任一階段失敗（sources INSERT / chapters DELETE / chapters re-INSERT / progress UPSERT）
- **When** dao return Err
- **Then** 整 tx rollback、三表狀態不變
- **And** caller 收 propagated Err

**Scenario 4：SwitchSourceDeps trait 簽名同步**
- **Given** `SwitchSourceDeps::switch_source_tx` trait method（testability seam）
- **When** trait 重新定義
- **Then** method 簽名與 facade 一致（加 `target_idx: Option<i64>`）
- **And** `RealDeps` production impl forward 不變
- **And** mock impl（既有 fault-injection UT 用）簽名同步

---

## REQ-004: Caller-aware Transition continuity

**描述**：picker 完成後依 `entry: EntryMode` 分派 transition 目標 — reader-entry 回 reader 跳該章、shelf-entry 回 shelf 帶 toast。Esc 取消亦對齊。

### Scenarios

**Scenario 1：reader-entry confirm 成功**
- **Given** SearchPickerScreen 由 reader 入口建立（entry = `PickerEntry::Reader { previous_chapter_idx: i64 }`）
- **When** confirm 成功、`switch_source_core::run` 回 SwitchOutcome
- **Then** picker transition `Transition::To(Box::new(ReaderScreen::new(EntryMode::Direct, ctx, novel_id).await?))` — reuse 既有 ReaderScreen ctor、ctor 內 `library::facade::get_progress` 讀回 switch_source_tx 已寫入的新 `chapter_index`
- **And** 新 ReaderScreen 進場後 `buffer.curr_chapter_idx == outcome.new_progress_idx`（透過 DB progress 同步，不需新加 `with_chapter` ctor）
- **And** reader-entry 路徑 user 操作步數 = s → Enter → y → 自動進新 reader（共 3 鍵 + 自動 transition）

**Scenario 2：shelf-entry confirm 成功**
- **Given** SearchPickerScreen 由 shelf 入口建立（entry = `EntryMode::Shelf`）
- **When** confirm 成功
- **Then** picker transition `Transition::To(ShelfScreen + toast「已換源 <old_src> → <new_src>，目標：第 <new_progress_idx + 1> 章 《<new_progress_chapter_name>》」)`
- **And** toast TTL 3 秒（reuse shelf-delete pattern）

**Scenario 3：Esc 取消依 entry 回原 caller**
- **Given** picker Picking 或 Confirming phase
- **When** 使用者按 Esc
- **Then** entry=Reader → `Transition::To(ReaderScreen)` 重 reader（不傳 new_progress_idx、回到 progress 既存值）
- **And** entry=Shelf → `Transition::To(ShelfScreen)` 不帶 toast、保留原 highlight

---

## REQ-005: 入口 mode-aware（shelf / reader Normal）

**描述**：shelf 按 s 從 highlight novel 取資料；reader Normal mode 按 s 從自身 buffer 取資料；reader Filter mode 按 s 視為 query 字元、不開 picker。

### Scenarios

**Scenario 1：shelf 按 s 進 picker**
- **Given** shelf 有書、使用者 highlight 某 row
- **When** 按 s
- **Then** 取當前 novel_id + name + author（`library::facade::list_shelf` 已有）+ progress.chapter_index + `library::facade::get_chapter(novel_id, idx).name`
- **And** transition `SearchPickerScreen::new(EntryMode::Shelf, novel_id, name, author, current_idx, current_chapter_name)`

**Scenario 2：shelf 空 / 無 highlight**
- **Given** shelf 為空或 ListState selected 為 None
- **When** 按 s
- **Then** no-op、不開 picker、Transition::Stay

**Scenario 3：reader Normal mode 按 s**
- **Given** ReaderScreen 在 Normal mode（mode == ReaderMode::Normal）
- **When** 按 s
- **Then** 取 reader.novel_id + chapters[reader.current as usize].name 為 anchor 章名 + reader.current as i64 為 anchor
- **And** transition `SearchPickerScreen::new(EntryMode::Reader { previous_chapter_idx: reader.current }, novel_id, name, author, reader.current, current_chapter_name)`

**Scenario 4：reader Filter mode 按 s**
- **Given** ReaderScreen 在 Filter mode（mode == ReaderMode::Filter）
- **When** 按 `KeyCode::Char('s')`
- **Then** s 被 append 到 query（既有 Filter mode Char 處理路徑）
- **And** picker 不開、reader.mode 不變

---

## REQ-006: Regression invariant

**描述**：apply_fuzzy_filter 簽名擴後既有 5 個 caller（2 production + 3 test asserts）行為不退化；switch_source_core fault-injection UT 與 CLI 行為（target_idx=None）不退化；既有 reader Filter mode 5 UT 不退化；shelf-delete + TUI reader 進階體驗 118 tests 全綠。

### Scenarios

**Scenario 1：apply_fuzzy_filter 既有 production caller 不退化**
- **Given** reader.rs:862 與 :902 兩個 production caller 原使用 `Vec<usize>` return
- **When** 簽名擴為 `(query, chapters, anchor: Option<i64>) -> Vec<(usize, i64)>`
- **Then** caller 改為 `.into_iter().map(|(i,_)| i).collect()` wrapper
- **And** Filter mode fuzzy filter list 顯示順序與本次擴前等價（既有 INT-mode-02/03/04 + int_toggle_02 五 UT 全綠）

**Scenario 2：apply_fuzzy_filter 既有 test asserts cascade**
- **Given** reader.rs:1938/1957/1959 三處 test asserts 原 `assert_eq!(result, vec![1])` 與 `r1.contains(&0)`
- **When** 簽名擴後
- **Then** 三處 assert 改 wrapper 包覆、行為斷言一致（contains / vec match 不變）

**Scenario 3：switch_source_core fault-injection UT cascade**
- **Given** switch_source_core 既有 fault-injection UT（5 個 abort case：FetchInfo / FetchToc / TocEmpty / SourceUpsert / SwitchTx）原假設 target_idx=None
- **When** trait method + run 簽名擴
- **Then** 既有 5 UT call site 加 `None` 參數、行為不變
- **And** 新加一條 UT 驗 `target_idx=Some(N)` 路徑：mock toc 5 章、target_idx=Some(3) → SwitchOutcome.new_progress_idx == 3、new_progress_chapter_name == mock_toc[3].name

**Scenario 4：CLI 兼容**
- **Given** 既有 CLI handler `presentation/handlers/switch_source.rs` 呼 `switch_source_core::run`
- **When** trait 加 target_idx 參數
- **Then** CLI 傳 `None`、output 訊息與本次擴前等價（「進度重置到第 1 章: <first chapter name>」）

**Scenario 5：shelf-delete + TUI reader 進階體驗 regression**
- **Given** 既有 118 tests（含 INT-trait / INT-buffer / INT-viewport / INT-scroll / INT-jump / INT-progress / INT-boundary / INT-toggle / INT-mode / INT-hit / INT-mouse-* / int_filter_03 / int_trait_*）
- **When** 本次擴改動完成
- **Then** `cargo test` 顯示 118 baseline + 新 INT 全綠、0 failed
- **And** baseline 3 條 dead_code warning 維持（select_within / BackupReceipt.filename / SwitchOutcome.chapter_count；後者本次擴可能消失因 SwitchOutcome 加新欄位被 toast 用）

---

## 對應 Criteria → REQ 追溯

| Criteria | REQ |
|---|---|
| C1 並行 streaming 搜尋 | REQ-001 S1/S2/S3 |
| C2 Enter 提早選 | REQ-001 S4 |
| C3 Fuzzy 對應預覽 | REQ-002 S1/S2/S3 |
| C4 Atomic 換源 | REQ-003 S1/S3 |
| C5 Caller-aware Transition | REQ-004 全 |
| C6 Reader Filter mode 不誤觸 | REQ-005 S4 |
| C7 cargo test 全綠 + cascade 不退化 | REQ-006 全 |
| C8 手動驗收 | E2E（test.md 會列） |

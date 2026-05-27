# Test Strategy

本次測試策略：picker UX 走整合測試 + ratatui TestBackend；fuzzy 純函數抽 free fn 直接驗；atomicity 沿用既有 fault-injection UT pattern + 新加 target_idx 路徑；視覺類 E2E 留手動 `cargo install` 後操作 binary 驗收。

---

## E2E 測試策略

E2E 走手動（既有 invariant — 此專案無 headless TUI 自動測 framework）。下表場景必須在 /baransu:execute 完成、merge 前由使用者手動驗收。

| 場景 | 起點 | 終點 | 對應 Criteria |
|------|------|------|--------------|
| **E2E-01：shelf 入口完整 flow** | shelf 開、highlight 某書 | 按 s → picker 看 streaming → Enter → Confirming → y | C1 / C2 / C3 / C4 / C5（shelf-entry 回 shelf 帶 toast） |
| **E2E-02：reader 入口 continuity** | reader 開、讀某長篇某中段章 | 按 s → picker → Enter → y → 自動進新 ReaderScreen 同章名對應位置 | C5（reader-entry continuity 保留）+ C4 |
| **E2E-03：Filter mode 不誤觸** | reader 開、`/` 進 Filter mode | 按 `s` → s 進 query 字元、picker 不開 | C6 |
| **E2E-04：timeout 書源不阻塞** | enable 10+ 書源（含 1-2 個慢 / 已 down） | 按 s → 觀察快書源先回、慢書源 5s 後灰色「逾時」、UI 不卡 | C1 |
| **E2E-05：score < 50 abort** | 換源到「同人作 / 章節名完全不同的書」 | Confirming 顯「找不到對應章節」、Esc 回 Picking、不寫 DB | C3 |
| **E2E-06：CLI 兼容** | CLI `novel-looker switch-source <id>` 既有指令 | 換源後進度顯示「重置到第 1 章」（未動行為） | C7 |
| **E2E-07：fuzzy 閾值收尾驗收** | 跑 throwaway script 對 3-5 本實書 dump CSV | user 看 score 分布、決定 50 保留 / 調整、寫回 const | Unknown 2 解決 |

---

## 整合測試策略

整合測試 = Rust UT 跨多模組（picker + library facade + switch_source_core + apply_fuzzy_filter + DB）。in-memory SQLite + mock SearchLike / SwitchSourceDeps。

### REQ-001 / SearchPickerScreen UX

| 測試目標 | 涉及層 | 關鍵驗證點 |
|---------|--------|-----------|
| **INT-picker-spawn-01：spawn N tasks** | picker + SearchLike mock | shelf 按 s 進 picker、mock SearchLike 註冊 3 個書源 → JoinSet spawn 3 task；MockSearchLike call_count == 3 |
| **INT-picker-stream-02：streaming append** | picker + SearchLike mock with delays | 3 個書源不同延遲 (100ms / 500ms / 1000ms) → results 按完成順序 append；100ms 那行最早出現 |
| **INT-picker-timeout-03：5s timeout** | picker + SearchLike mock with 6s delay | 該書源 6s delay → 5s 過 timeout 觸發、SearchResult.status = Timeout、其他書源不影響 |
| **INT-picker-enter-04：Enter abort_all + Phase 切換** | picker handle_event | at least 1 Ok row 狀態下 Enter → 內部 `JoinSet::abort_all` 被呼（mock 驗 task cancel）、phase 從 Picking 切到 Confirming{ selected_idx } |
| **INT-picker-enter-pending-row-05：選 Loading 行 no-op** | picker handle_event | selected row status=Loading → Enter no-op、phase 不切 |
| **INT-picker-esc-cancel-shelf-06：shelf-entry Esc** | picker handle_event | entry=Shelf、Esc → Transition::To(ShelfScreen)、不寫 DB（mock DB no UPDATE 呼） |
| **INT-picker-esc-cancel-reader-07：reader-entry Esc** | picker handle_event | entry=Reader{prev_idx=10}、Esc → Transition::To(ReaderScreen)、不寫 DB |
| **INT-picker-empty-shelf-08：shelf 空時按 s** | shelf handle_event | shelf ListState selected=None → 按 s → Transition::Stay、picker 不開 |
| **INT-picker-confirming-pending-transition-09：Confirming async polling 機制** | picker handle_event + draw | Enter 進 Confirming { sync_state: Pending }、第 1 frame draw 顯「準備換源、抓取新源 TOC 中...」；mock sync_task 完成（fetch_toc + fuzzy）後下 1 frame draw 顯 Ok / Abort 對應訊息；驗 Pending → Ok / Abort state transition 真實發生 |
| **INT-picker-draw-01：TestBackend round-trip 表格與顏色** | picker draw + ratatui TestBackend | `TestBackend::new(80, 24)` + `Terminal::draw(\|f\| picker.draw(f, &ctx))` 跑 1 frame；驗 表格行數 == enabled 書源數、Loading 行顯白色、Timeout 行顯灰色、Ok 行顯正常色（對應 Seam 5）|

### REQ-002 / Fuzzy mapping with anchor

| 測試目標 | 涉及層 | 關鍵驗證點 |
|---------|--------|-----------|
| **INT-fuzzy-anchor-01：tiebreak proximity** | apply_fuzzy_filter | chapters = mock 5 章；query 同時命中 chapter idx 1 與 4 同 score；anchor=Some(2) → 結果第一筆是 idx=1（proximity 1 vs 2） |
| **INT-fuzzy-anchor-02：anchor=None stable order** | apply_fuzzy_filter | 同上但 anchor=None → 結果第一筆是命中順序（stable） |
| **INT-fuzzy-cjk-mapping-01：跨書源中文章名** | apply_fuzzy_filter | 舊章「第 47 章 出關」、新源 toc 含「第 48 章 出關」、「第 100 章 出關」、anchor=Some(47) → best = idx of 「第 48 章」 |
| **INT-fuzzy-threshold-01：score = 50 拒絕** | apply_fuzzy_filter + picker Confirming logic | mock fuzzy 回 best score 50 → SyncState::Abort{ FuzzyBelow(50) }、Confirming 顯「找不到對應章節 (best score 50 ≤ 50)」 |
| **INT-fuzzy-threshold-02：score = 51 接受** | 同上 | best score 51 → SyncState::Ok、Confirming 顯預覽允許 y |
| **INT-fuzzy-cascade-reader-01：reader.rs:862 caller** | apply_fuzzy_filter + reader Filter mode | 既有 INT-mode-02 用 wrapper `.into_iter().map(|(i,_)| i).collect()` 仍綠（**與 REQ-006 表內 INT-regression-fuzzy-callers-01 共用同一批既有 5 UT、由不同 REQ 視角追溯：REQ-002 視角為「功能擴後仍正確」、REQ-006 視角為「regression 不退化」**）|
| **INT-fuzzy-cascade-reader-02：reader.rs:902 caller** | 同上 | 既有 INT-mode-03 / INT-mode-04 / int_toggle_02 仍綠（命名等價於 REQ-006 表的 INT-regression-fuzzy-asserts-02）|
| **INT-edge-empty-toc-01：picker Confirming 新源 toc=0** | picker Confirming + switch_source_core::run | mock fetch_toc 回 empty Vec → run 內 evaluate_toc 回 AbortReason::EmptyToc → picker sync_state = Abort { EmptyToc }、draw 顯「新源無章節 (best score N/A)」、Esc 回 Picking phase、不寫 DB |

### REQ-003 / Atomic switch contract 擴

| 測試目標 | 涉及層 | 關鍵驗證點 |
|---------|--------|-----------|
| **INT-switch-target-some-01：寫入指定 idx** | switch_source_tx + dao | 給 target_idx=Some(3)、toc 5 章 → dao step 4 寫 progress.chapter_index = 3；SELECT progress WHERE novel_id 驗 |
| **INT-switch-target-none-02：fall back first_idx** | 同上 | 給 target_idx=None、toc 第一章 idx=0 → progress.chapter_index = 0；行為與本次擴前等價 |
| **INT-switch-outcome-name-01：new_progress_chapter_name** | switch_source_core::run | target_idx=Some(3)、toc[3].name="第 4 章 試煉" → SwitchOutcome.new_progress_chapter_name == "第 4 章 試煉" |
| **INT-switch-outcome-name-02：target=None 仍對齊 first** | 同上 | target_idx=None、toc[0].name="序章" → new_progress_chapter_name == "序章" |
| **INT-switch-deps-trait-01：SwitchSourceDeps 簽名擴** | switch_source_core + trait | RealDeps impl 與 Mock impl trait method 簽名都加 target_idx → 編譯通過、既有 5 fault-injection UT call site 加 None 不退化 |
| **INT-switch-tx-rollback-01：fault injection cascade** | switch_source_tx + mock fault | 既有 5 個 abort case (FetchInfo / FetchToc / TocEmpty / SourceUpsert / SwitchTx) 加 target_idx=None call → 全綠不退化 |
| **INT-switch-target-out-of-bounds-01：defensive fall back (上越界)** | switch_source_core::run | target_idx=Some(99)、toc 5 章 (99 越界) → 視為 None fall back first_idx；不 panic |
| **INT-switch-target-out-of-bounds-02：defensive fall back (負數)** | switch_source_core::run + library facade | target_idx=Some(-1)、toc 5 章 → 視為 None fall back first_idx；不 panic；progress.chapter_index 寫 0 |

### REQ-004 / Caller-aware Transition

| 測試目標 | 涉及層 | 關鍵驗證點 |
|---------|--------|-----------|
| **INT-transition-reader-confirm-01：reader-entry y confirm** | picker confirm path + `next_transition` free fn | entry=Reader{prev_idx=10}、outcome.new_progress_idx=15 → Transition::To(ReaderScreen with novel_id + initial chapter 15) |
| **INT-transition-shelf-confirm-02：shelf-entry y confirm** | 同上 | entry=Shelf、outcome.new_progress_idx=15、outcome.new_progress_chapter_name="X" → Transition::To(ShelfScreen) + toast 含「目標：第 16 章 《X》」 |
| **INT-transition-esc-reader-03：reader-entry Esc** | picker Esc | entry=Reader、Esc → Transition::To(ReaderScreen)（不傳 new chapter、reader 從 DB get_progress 回原章） |
| **INT-transition-esc-shelf-04：shelf-entry Esc** | 同上 | entry=Shelf、Esc → Transition::To(ShelfScreen)、無 toast |

### REQ-005 / 入口 mode-aware

| 測試目標 | 涉及層 | 關鍵驗證點 |
|---------|--------|-----------|
| **INT-entry-shelf-build-01：shelf 's' picker 建構** | shelf handle_event | shelf highlight idx=2、novel_id=5、`library::facade::get_chapter(5, 47).name="出關"` → picker.book_name = list_shelf[2].name、author = list_shelf[2].author、old_chapter_idx = 47、old_chapter_name = "出關" |
| **INT-entry-reader-normal-build-02：reader Normal 's' picker 建構** | reader handle_event | reader.novel_id=5、reader.current=47、chapters[47].name="出關" → picker.book_name 同、old_chapter_idx=47、old_chapter_name="出關"、entry=Reader{prev_idx=47} |
| **INT-entry-reader-filter-no-op-03：reader Filter 's' query 字元** | reader handle_event | reader.mode = Filter{ query="入" }、按 `s` → query 變 "入s"、reader.mode 仍 Filter、不 transition |
| **INT-entry-shelf-empty-04：shelf 空** | shelf handle_event | shelf 空、按 s → Transition::Stay、無 picker |
| **INT-entry-shelf-chapter-not-found-05：舊章 row 不存在** | shelf handle_event + library::facade::get_chapter | seed shelf 有書、progress.chapter_index=99（chapters table 內無 idx=99 row）→ 按 s → `get_chapter` 回 None → 不開 picker、Transition::To(ShelfScreen::with_highlight_until(.., Some("找不到舊章節，無法換源"), TTL)) |
| **INT-entry-reader-chapter-not-found-06：reader-entry 舊章 row 不存在** | reader handle_event | reader.current=99 但 chapters.len()=50 → 按 s → 不開 picker、reader.toast = "找不到舊章節，無法換源"、reader.toast_expires_at = Some(.. + TOAST_TTL) |

### REQ-006 / Regression

| 測試目標 | 涉及層 | 關鍵驗證點 |
|---------|--------|-----------|
| **INT-regression-fuzzy-callers-01：apply_fuzzy_filter 既有 2 production caller cascade** | reader Filter mode | INT-mode-02 / INT-mode-03 / INT-mode-04 / int_filter_03_* / int_toggle_02 5 個 UT 全綠 |
| **INT-regression-fuzzy-asserts-02：3 處 test asserts cascade** | reader UT | reader.rs:1938/1957/1959 三處 assert 改 wrapper 後行為斷言一致 |
| **INT-regression-switch-deps-03：SwitchSourceDeps 5 fault-injection cascade** | switch_source_core fault-injection 既有 | 既有 5 個 abort case UT call site 加 None、全綠 |
| **INT-regression-cli-04：CLI handler target=None 行為** | CLI presentation/handlers/switch_source.rs | CLI handler 走 None 路徑、output 訊息「進度重置到第 1 章: <first chapter name>」與本次擴前等價 |
| **INT-regression-baseline-118-05：cargo test 全綠** | 全 module | cargo test 整 suite 118 + 新增 INT ≈ 30+ → 全綠、0 failed |

---

## 關鍵邊界條件

以下邊界條件必須有 UT 覆蓋（連結對應需求）：

### Picker UX (REQ-001)
- 0 個 enabled 書源（picker 表格空）→ 任何 Enter no-op；picker draw 顯「無 enabled 書源、請先 source import」advisory 文案（INT-picker-empty-source-list-09 — 補實作後加 UT；目前 advisory）
- 所有書源 timeout（picker 全灰）→ Enter no-op（由 INT-picker-timeout-03 與 INT-picker-enter-pending-05 共同覆蓋）
- selected row Loading → Enter no-op（INT-picker-enter-pending-row-05）
- picker drop（Esc 或 Transition）期間 pending JoinSet tasks 行為 — Enter 路徑由 `INT-picker-enter-04` 驗 `abort_all()` 顯式呼用；Esc / Transition 路徑由 `JoinSet` Drop semantics 自動 abort（tokio 1.x 保證、無需 explicit UT；對應原 Unknown 1 spike 結果若是 cancel-safe 則此邊界 closed、若非則 KD1 重新評估）
- 舊章 `library::facade::get_chapter(novel_id, old_idx)` 回 None caller-side guard（INT-entry-shelf-chapter-not-found-05 + INT-entry-reader-chapter-not-found-06，REQ-005 表）

### Fuzzy mapping (REQ-002)
- score = 50 邊界（嚴格 > 才接受、INT-fuzzy-threshold-01）
- score = 51 邊界（接受、INT-fuzzy-threshold-02）
- anchor=Some 同分 tiebreak（INT-fuzzy-anchor-01）
- anchor=None stable order（INT-fuzzy-anchor-02）
- 舊章名查不到 chapters table → 不進 picker、回 caller 帶 toast（INT-entry-shelf-build / reader-build 前置 Err 路徑、需新加）
- 新源 toc=0（INT-edge-empty-toc）— 由 switch_source_core::run 既有 EmptyToc abort 涵蓋
- target_idx 越界 → fall back first_idx（INT-switch-target-out-of-bounds-01）

### Atomicity (REQ-003)
- target_idx=Some(N) 寫入路徑（INT-switch-target-some-01）
- target_idx=None CLI 路徑（INT-switch-target-none-02）
- 5 個 abort case fault-injection 不退化（INT-regression-switch-deps-03）

### Caller-aware Transition (REQ-004)
- reader-entry Esc 不傳 new_progress_idx（INT-transition-esc-reader-03 — reader 從 DB get_progress 回原章）
- shelf-entry Esc 無 toast（INT-transition-esc-shelf-04）
- 越界 ReaderScreen::new_with_chapter 失敗（chapter idx 不存在）→ ReaderScreen 既有 init_buffer Err 路徑 propagate、上層 catch（reuse 既有 ChapterBuffer init_buffer B1 邊界）

### 入口 mode-aware (REQ-005)
- reader Filter mode 按 s = query 字元（INT-entry-reader-filter-no-op-03、覆蓋 C6）
- shelf 空（INT-entry-shelf-empty-04）

### Regression (REQ-006)
- apply_fuzzy_filter 5 個既有 caller cascade
- switch_source_core 5 個 fault-injection cascade
- CLI handler 兼容
- 118 baseline 全綠

---

## 測試覆蓋追溯表（REQ → UT/E2E）

| REQ | UT | E2E |
|---|---|---|
| REQ-001 picker UX | INT-picker-spawn-01 / stream-02 / timeout-03 / enter-04 / enter-pending-05 / esc-shelf-06 / esc-reader-07 / empty-shelf-08 / confirming-pending-transition-09 / draw-01 | E2E-01 / E2E-04 |
| REQ-002 fuzzy mapping | INT-fuzzy-anchor-01/02 / cjk-mapping-01 / threshold-01/02 / cascade-reader-01/02 / edge-empty-toc-01 | E2E-05 |
| REQ-003 atomic switch | INT-switch-target-some-01 / target-none-02 / outcome-name-01/02 / deps-trait-01 / tx-rollback-01 / target-out-of-bounds-01/02 | E2E-01 / E2E-02 / E2E-06 |
| REQ-004 caller-aware Transition | INT-transition-reader-confirm-01 / shelf-confirm-02 / esc-reader-03 / esc-shelf-04 | E2E-01 / E2E-02 |
| REQ-005 入口 mode-aware | INT-entry-shelf-build-01 / reader-normal-build-02 / reader-filter-no-op-03 / shelf-empty-04 / shelf-chapter-not-found-05 / reader-chapter-not-found-06 | E2E-03 |
| REQ-006 regression | INT-regression-fuzzy-callers-01（≡ INT-fuzzy-cascade-reader-01）/ fuzzy-asserts-02（≡ INT-fuzzy-cascade-reader-02）/ switch-deps-03 / cli-04 / baseline-118-05 | E2E-06 / 隱含全部 |

每個 REQ 至少 1 個 UT + 1 個 E2E 場景；REQ-006 regression 由 baseline UT + 既有手動 binary smoke 覆蓋。

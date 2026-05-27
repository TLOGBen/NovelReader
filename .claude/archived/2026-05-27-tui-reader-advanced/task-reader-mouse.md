# Tasks: reader-mouse
**前置群組**：infra（trait sig 已支援 Event::Mouse）+ reader-buffer（跳章 + buffer rebuild API + ReaderScreen struct 已 stub toc_width_cached 欄位）

> Mouse wheel + click hit-test。在 infra 完成後 reader 已收得到 Event::Mouse，本 group 把 Stay 改成實際動作。**理論可與 reader-toc group 並行，但實務建議串行**（兩 group 都動 reader.handle_event 的 match arms，並行 rebase 仍可能小衝突；buffer-02 已把所有 struct 欄位一次 stub 完，merge 風險已降至最低）。

---

## TASK-reader-mouse-01: hit_test_pane helper + ReaderScreen 記錄 toc_width

**需求追溯**：REQ-004 / REQ-005
**目標**：實作 `hit_test_pane(column, toc_width) -> Pane` 純函數；draw() 時把當前 toc_width 記到 reader state（同 content_area_h pattern）供 mouse handler 取用。

**驗收標準**：
- [ ] enum Pane { Toc, Content }（reader.rs private）
- [ ] `pub(crate) fn hit_test_pane(column: u16, toc_width: u16) -> Pane` 純 free fn（design.md Seam 3）
- [ ] `pub(crate) fn hit_test_toc_row(row: u16, list_offset: u16, items_count: usize) -> Option<usize>` 純 free fn（避免 1-caller 問題：UT 是第 2 個 caller）
- [ ] reader.draw() 每次更新 `toc_width_cached`（欄位已在 buffer-02 stub）
- [ ] UT INT-hit-01/02/03/04 全綠（含 toc_width 用 ratatui TestBackend 跑一次 draw → 驗 cached value，design.md Seam 4）

### 步驟

- [ ] 加 enum Pane
- [ ] 加 fn hit_test_pane
- [ ] reader.draw() 算完 toc_width 後存到 state
- [ ] UT 三個邊界（在 TOC、在 content、TOC collapsed = 整面 content）

---

## TASK-reader-mouse-02: Wheel scroll handler (pane-aware speed)

**需求追溯**：REQ-004
**目標**：Event::Mouse(MouseEvent { kind: ScrollUp | ScrollDown, column, .. }) 在 reader 內按 hit_test_pane 決定 step：TOC pane = ±1 章（跳章 + rebuild）；content pane = ±3 行（scroll only）。

**驗收標準**：
- [ ] Normal mode、Mouse ScrollDown + 在 TOC pane → current ± 1、rebuild_buffer、scroll = prev_end_row of new buffer
- [ ] Normal mode、Mouse ScrollDown + 在 content pane → scroll += 3、檢查邊界
- [ ] ScrollUp 同上反向
- [ ] toc_collapsed 時整面當 content
- [ ] Filter mode + 在 TOC pane Wheel → selected ± 1 in filtered_indices（不跳章、不改 reader.current）
- [ ] inline UT 覆蓋（mock MouseEvent 餵 handle_event）
- [ ] INT-mouse-filter-01：mode=Filter、TOC pane Wheel → selected ± 1 in filtered_indices、reader.current 不變

### 步驟

- [ ] handle_event match Event::Mouse(me) → 根據 me.kind 與 hit_test 分派
- [ ] 注意 Filter mode 期間的特殊行為（Wheel 走 filtered_indices）
- [ ] UT

---

## TASK-reader-mouse-03: Click TOC 跳章

**需求追溯**：REQ-005
**目標**：Event::Mouse(MouseEvent { kind: Down(Left), column, row, .. }) 在 TOC pane → 換算 row → chapter idx → 跳章；在 content pane → no-op；點空白 row → no-op。

**驗收標準**：
- [ ] Mouse Down(Left) + 在 TOC pane + row 對應某 chapter（按 ListState 計算）→ 跳該章（rebuild_buffer + scroll = prev_end_row）
- [ ] Mouse Down(Left) + 在 content pane → no-op、不改 state
- [ ] Mouse Down(Left) + TOC pane 但 row 超出 chapters 長度 → no-op
- [ ] Filter mode + 點 filtered list 某 row → 跳該章（用 filtered_indices[row] 算 real idx）、退出 filter mode
- [ ] **INT-jump-01b**：mock Event::Mouse(Down(Left), column, row) 餵 reader.handle_event → 觸發跳章（呼 apply_jump_to）、行為一致；buffer-02 的 INT-jump-01a 已驗 apply_jump_to 內部邏輯，本 UT 驗 click 入口 → apply_jump_to 的 dispatch 路徑

### 步驟

- [ ] 加 helper `fn hit_test_toc_row(row: u16, toc_list_state_offset: u16, items_count: usize) -> Option<usize>` 把 row → list idx
- [ ] handle_event 加 Mouse Down(Left) arm
- [ ] Filter mode 的 click 邏輯
- [ ] UT INT-jump-01

### unknown #3 處置

「滑鼠 ScrollUp/Down 是否連動 ListState.select() 移動 highlight」— 在 TASK-reader-mouse-02 實作 wheel 時，TOC pane wheel 就是「移動 highlight + 跳章」，本身就連動 ListState.select() 與 reader.current。content pane wheel 完全不動 TOC highlight。決策即「TOC wheel 連動 + 跳章；content wheel 只改 scroll」。

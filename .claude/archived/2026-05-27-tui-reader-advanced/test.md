# Test Strategy

本輪測試策略：infra change 走純 UT（既有 UT 不可退化）；reader 重寫走 UT + 手動驗收；mouse + 視覺類功能難自動化，靠 UT 驗 hit-test 邏輯 + 手動 cargo install 驗收。

---

## E2E 測試策略

E2E 在此專案 = 手動跑 `cargo install --path . --force` 後操作 binary。沒有 headless TUI 自動測試 framework。下表的場景必須在 `/baransu:execute` 完成、merge 前由使用者手動驗收。

| 場景 | 起點 | 終點 | 對應 Criteria |
|------|------|------|--------------|
| **E2E-01：無縫跨章感** | 開 reader 進入長篇某章中段 | 持續按 `J`（page down）穿越 1 章邊界 | C1：視覺上連續、無 loading、進度條 X 變 |
| **E2E-02：TOC toggle** | reader 開啟 | 按 `t` 兩次 | C2：30% → 0% → 30%、content 寬度同步變 |
| **E2E-03：fuzzy filter 跳章** | reader 開啟、TOC 有 100+ 章 | 按 `/`、輸入「入魔」、Enter | C3：TOC 即時 filter；Enter 跳到第一筆命中 |
| **E2E-04：filter cancel** | filter mode 中 | 按 `Esc` | C3：query 清空、TOC 恢復完整、不跳章 |
| **E2E-05：滑鼠滾輪 content** | reader 開啟、滑鼠在右側 | 滾輪向下 | C4：scroll 增加 3 行 |
| **E2E-06：滑鼠滾輪 TOC** | reader 開啟、滑鼠在左側 TOC | 滾輪向下 | C4：TOC highlight ±1 章、跳章 |
| **E2E-07：點 TOC 跳章** | reader 開啟、TOC 顯示 | 左鍵點某章 row | C5：跳該章、buffer rebuild、scroll 到該章開頭 |
| **E2E-08：trait migration regression** | 既有所有 screen | 按舊 binding（j/k/Enter/Esc/q/s/d/m/Tab） | C6：每個 binding 行為與本輪前完全一致 |

---

## 整合測試策略

整合測試 = Rust UT 跨多個內部模組（reader + library facade + ChapterMeta 資料模型 + ratatui 元件互動）。in-memory SQLite + 假 ChapterMeta 列。

| 測試目標 | 涉及層 | 關鍵驗證點 |
|---------|--------|-----------|
| **INT-buffer-01：rebuild_buffer 正常 3 章** | reader + library::facade + LibraryDb | fetch 三章成功 → buffer.combined_text 有三段 + offset 表正確 |
| **INT-buffer-02：rebuild_buffer 第 0 章降級** | 同上 | curr=0 → buffer 只有 curr + next，prev_chapter_idx = None |
| **INT-buffer-03：rebuild_buffer 最後章降級** | 同上 | curr=N-1 → buffer 只有 prev + curr，next_chapter_idx = None |
| **INT-buffer-04：rebuild_buffer cache hit 不重 fetch** | reader + library | 三章 content 都已存於 chapters.content → 不呼叫 scraper |
| **INT-buffer-04b：cache miss 後 save_chapter_content 寫回** | reader + library + catalog Scraper（mock） | cache miss → 呼 scraper → save_chapter_content → chapters.content 從 NULL 變非 NULL |
| **INT-buffer-05：rebuild_buffer next fetch 失敗降級** | reader + catalog Scraper（mock） | next chapter fetch Err → buffer 變 2 章、不 panic |
| **INT-buffer-06：rebuild_buffer 對 curr fetch Err 回 CurrFailed** | reader + ScraperLike（mock） | 注入 mock ScraperLike 對 curr 章 return Err → 呼 rebuild_buffer 回 `Err(RebuildError::CurrFailed { idx, .. })`；caller TASK-reader-buffer-04 負責把這個轉成 toast |
| **INT-jump-03：handle_event 對 CurrFailed 的處理** | reader handle_event | mock 注入觸發 rebuild → 接到 CurrFailed → reader.buffer / scroll / current 對 snapshot 完全不變、reader.toast = "載入第 N 章失敗"、TTL 3 秒 |
| **INT-progress-01：progress_text free fn** | progress_text 純 fn | 呼 `progress_text(buffer, scroll, total)` 用 mock buffer + scroll 在 prev/curr/next 三區 → 回字串 "第 X 章 / 共 N 章 (Y%)" 其中 X = viewport_top_chapter+1, Y = X*100/N |
| **INT-boundary-empty-chapters：B1 邊界** | reader::new + init_buffer | mock library::facade::list_chapters 回 `[]` → init_buffer 立即 Err 帶訊息「無章節可讀」；reader::new 把 Err 往上 propagate |
| **INT-boundary-empty-content：B2 邊界** | reader + ScraperLike | mock scraper 對某章 return Ok("") → buffer.combined_text 含 placeholder「（本章空白）」、prev_end_row / curr_end_row 正確、progress_text 不炸 |
| **INT-boundary-recursive-rebuild：B3 邊界** | reader handle_event scroll | 第 0 章 reader、scroll=0、收到向上 scroll → scroll 仍 0、無 rebuild call、無 panic；對稱對最後章向下 scroll |
| **INT-mode-01：filter mode 轉換** | reader 內部 state machine | Normal → 按 `/` → Filter{ query="" } → 按字元 → Filter{ query="X" } → Esc → Normal |
| **INT-mode-02：filter 篩選正確性** | reader fuzzy helper | chapters = [「第一章」, 「第二章 入魔」, 「第三章」] + query="入魔" → filtered_indices = [1] |
| **INT-mode-03：fuzzy CJK 命中** | fuzzy-matcher + reader | query="123" 命中「第123章 XXX」、query="入魔" 命中「第50章 入魔之路」 |
| **INT-mode-04：Backspace 空 query 無 panic + filter 進入時 TOC 強制展開** | reader handle_event | query=""、Backspace → query 仍 ""、no panic；toc_collapsed=true、按 / → toc_collapsed 變 false；Esc 退 filter 後 toc_collapsed 仍 false（不還原）|
| **INT-mouse-filter-01：filter mode + TOC pane Wheel 走 filtered_indices** | reader handle_event Mouse | mode=Filter、filtered_indices=[1,5,9]、selected=0、Mouse ScrollDown @ TOC → selected=1、reader.current 不變、不觸發 rebuild |
| **INT-hit-01：hit_test_pane TOC** | reader hit_test fn | column=10, toc_width=30 → Pane::Toc |
| **INT-hit-02：hit_test_pane content** | 同上 | column=50, toc_width=30 → Pane::Content |
| **INT-hit-03：hit_test_pane TOC collapsed** | 同上 | column=10, toc_width=0 → Pane::Content |
| **INT-hit-04：toc_width 從 draw 同步到 mouse handler** | reader draw + handle_event | reader.draw() 後 toc_width_cached 反映當前 toc_collapsed；mouse handler 用此值；row > items_count → hit_test_toc_row 回 None |
| **INT-trait-01：MenuScreen Event::Mouse → Stay** | tui/mod.rs Screen trait + MenuScreen | 對 menu 餵 Event::Mouse(ScrollUp) → Transition::Stay、state 不變 |
| **INT-trait-02：ShelfScreen Event::Mouse → Stay** | 同上 | 同上 for shelf |
| **INT-trait-03：DeleteConfirmScreen Event::Mouse → Stay** | 同上 | 同上 for delete_confirm（shelf-delete worktree merge 後） |
| **INT-trait-04：run_loop forward Mouse** | tui/mod.rs run_loop | mock 一個會收 Event 的 Screen → forward MouseEvent 正確 |
| **INT-trait-05：run_loop 對 Resize/Paste/FocusGained 走 continue** | tui/mod.rs run_loop | mock 收這三類 event → run_loop 不 forward 給 screen、不 panic、不破壞既有 forward 行為 |
| **INT-viewport-01：viewport_top_chapter 在 prev 區** | reader fn | scroll < prev_end_row → 回 prev_chapter_idx |
| **INT-viewport-02：viewport_top_chapter 在 curr 區** | 同上 | prev_end_row ≤ scroll < curr_end_row → 回 curr_chapter_idx |
| **INT-viewport-03：viewport_top_chapter 在 next 區** | 同上 | scroll ≥ curr_end_row → 回 next_chapter_idx |
| **INT-jump-01a：apply_jump_to(idx) 內部跳章邏輯** | reader 內部 free fn / method | 直接呼 apply_jump_to(5) → rebuild_buffer 被呼、toc_list_state.selected() == Some(5)、scroll = prev_end_row of new buffer、reader.current = 5 |
| **INT-jump-01b：點 TOC click 入口 → apply_jump_to dispatch** | reader handle_event Event::Mouse(Down) | mock Mouse Down(Left) at TOC pane row 5 → dispatch 到 apply_jump_to(5)；不重複驗 jump 內部邏輯（已由 01a） |
| **INT-jump-02：fuzzy Enter 觸發 rebuild** | reader handle_event Event::Key Enter in Filter mode | mock filtered_indices=[3, 7]、selected=0、Enter → rebuild_buffer(3) |
| **INT-scroll-01：滾過 next 結尾觸發 rebuild** | reader handle_event scroll | buffer 在邊界、再 scroll 1 行 → rebuild_buffer(viewport_top_chapter) |
| **INT-toggle-01：t 鍵切 toc_collapsed** | reader handle_event Event::Key 't' | toc_collapsed false → true → false |
| **INT-toggle-02：filter mode t 鍵不 toggle** | 同上 | mode=Filter、按 't' → query += 't'、toc_collapsed 不變 |

---

## 關鍵邊界條件

以下邊界條件**必須**有 UT 覆蓋（連結對應需求）：

### Trait migration（REQ-001）
- 6 個 screen 對 Event::Key 的回應與 migration 前一致（既有 UT 全部跑過）
- 6 個 screen 對 Event::Mouse 不報錯、返回 Stay（INT-trait-01/02/03 + reader 自己的 mouse handler）
- run_loop 不再吞 Event::Mouse、能 forward 給 screen（INT-trait-04）
- Event::Resize / Paste / FocusGained 仍走 `continue`（INT-trait-05；不在本輪 forward）
- 並行 fetch 策略屬實作細節，**不在 UT 範圍**（design.md 明標 unknown，由 execute 階段 benchmark 決定）

### TOC toggle（REQ-002）
- 初始 toc_collapsed = false（INT-toggle-01 中 inline）
- 連按 t 兩次回到原狀（同上）
- filter mode 期間 't' 不是 toggle（INT-toggle-02）

### Fuzzy filter（REQ-003）
- query 為空時 filtered_indices = all chapter indices（INT-mode-02 variant）
- 沒命中時 filtered_indices = []（INT-mode-02 variant）
- Enter 在 filtered_indices 空時不跳章不退出 filter（REQ-003 Scenario 7）
- Backspace 在 query 為空時不報錯
- CJK 命中（INT-mode-03）

### Mouse hit-test（REQ-004 / REQ-005）
- column = 0 在 TOC pane（INT-hit-01 變形）
- column = toc_width（邊界值）在 content pane（INT-hit-02 變形）
- toc_collapsed 時整個畫面 = content pane（INT-hit-03）
- 點 TOC list 空白 row（list 短於 area）→ no-op（design.md 錯誤處理已列）
- filter mode 點 TOC → 走 filtered_indices 不走全列表（REQ-005 Scenario 4）

### Eager buffer（REQ-006）
- 第 0 章開 reader → prev=None、buffer 兩章（INT-buffer-02）
- 最後章開 reader → next=None、buffer 兩章（INT-buffer-03）
- 跨邊界滾動觸發 rebuild、scroll 重算保留可視內容相對位置（INT-scroll-01）
- 跳章內部邏輯（INT-jump-01a）+ click 入口（INT-jump-01b）
- rebuild fetch 失敗降級不 panic、不破壞舊 buffer（INT-buffer-05 / INT-jump-03）
- viewport_top_chapter 在三個區的回值（INT-viewport-01/02/03）
- **B1：chapters 為空 → init_buffer Err**（INT-boundary-empty-chapters）
- **B2：章節 content 空字串 → placeholder**（INT-boundary-empty-content）
- **B3：scroll 越界 + prev/next=None → 鎖邊界不再 rebuild**（INT-boundary-recursive-rebuild）

### 與 shelf-delete 相容性
- delete_confirm.rs（shelf-delete 新檔）的 handle_event 簽名要跟 trait migration 一起改、UT 跟著修
- shelf.rs 的 toast TTL（shelf-delete 已加）+ d 鍵 wire（shelf-delete 已加）保留行為

---

## 測試覆蓋追溯表（REQ → UT/E2E）

| REQ | UT | E2E |
|---|---|---|
| REQ-001 trait migration | INT-trait-01/02/03/04/05 + 既有所有 screen UT | E2E-08（對應 design「Screen trait migration 影響範圍」表 KeyEvent 欄「既有不變」invariant） |
| REQ-002 TOC toggle | INT-toggle-01/02 + INT-mode-04（filter 強制展開部分） | E2E-02 |
| REQ-003 fuzzy filter | INT-mode-01/02/03/04 | E2E-03 / E2E-04 |
| REQ-003 S8 filter 進入強制展開 TOC | INT-mode-04（後半部分） | 含於 E2E-03 |
| REQ-004 mouse wheel | INT-hit-01/02/03/04 + INT-mouse-filter-01 + reader wheel handler UT | E2E-05 / E2E-06 |
| REQ-005 mouse click | INT-hit-01/02/03/04 + INT-jump-01a + INT-jump-01b | E2E-07 |
| REQ-006 Eager buffer | INT-buffer-01/02/03/04/04b/05/06 + INT-viewport-01/02/03 + INT-scroll-01 + INT-jump-01a/02/03 + INT-progress-01 + INT-boundary-empty-chapters + INT-boundary-empty-content + INT-boundary-recursive-rebuild | E2E-01 |

每個 REQ 至少有 1 個 UT + 1 個 E2E 場景覆蓋。

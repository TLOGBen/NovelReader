# Tasks: tui-picker
**前置群組**：handler-core + shared

> 新檔 picker.rs：SearchPickerScreen 完整 with JoinSet streaming + Phase enum + SearchLike trait + caller-aware Transition。拆兩 task：picker-01 是 Picking phase 骨架，picker-02 是 Confirming phase + 換源 wire。

---

## TASK-tui-picker-01: SearchPickerScreen 骨架 + SearchLike trait + Picking phase + JoinSet streaming

**需求追溯**：REQ-001 / REQ-004（Esc 取消路徑）
**目標**：新檔 picker.rs；定義 PickerEntry / Phase / SyncState / SearchResult / SearchStatus / SearchPickerScreen 完整資料模型；SearchLike trait + Scraper impl；Picking phase 完整：JoinSet spawn N + streaming append + timeout + Enter 切 Confirming + Esc caller-aware Transition。

**驗收標準**：
- [ ] 新檔 `src/presentation/handlers/tui/picker.rs`
- [ ] 定義 `pub(crate) enum PickerEntry { Reader { previous_chapter_idx: i64 }, Shelf }`
- [ ] 定義 `pub(crate) enum Phase { Picking, Confirming { selected_idx: usize, sync_state: SyncState } }`
- [ ] 定義 `pub(crate) enum SyncState { Pending, Ok { new_idx: i64, new_chapter_name: String, score: i64 }, Abort { reason: AbortKind }, Err { msg: String } }`
- [ ] 定義 `pub(crate) enum AbortKind { EmptyToc, FuzzyBelow(i64) }`
- [ ] 定義 `pub(crate) struct SearchResult { src_url: String, status: SearchStatus, hit: Option<SearchHit> }` + `pub(crate) enum SearchStatus { Loading, Ok, Timeout, Failed { msg: String } }`
- [ ] 定義 `pub(crate) struct SearchPickerScreen` 含 novel_id / book_name / author / old_chapter_idx / old_chapter_name / entry / phase / results / list_state / join_set
- [ ] 定義 `pub(crate) trait SearchLike` async fn search(source, keyword) -> Result<Vec<SearchHit>>
- [ ] production `impl SearchLike for catalog::service::scraper::Scraper`
- [ ] ctor `pub fn new(entry, novel_id, book_name, author, old_chapter_idx, old_chapter_name) -> Self`
- [ ] async fn `spawn_searches<S: SearchLike + 'static>(&mut self, scraper, enabled_sources)` JoinSet spawn N task per source、每個 task 含 5s timeout（tokio::time::timeout）
- [ ] Screen impl draw() Picking phase：標題列 + 表格 + ListState selected
- [ ] Screen impl handle_event Picking phase：
  - j/k 移 list_state
  - Enter（selected row status=Ok 才接受）→ join_set.take().abort_all() + phase 切 Confirming { selected_idx, sync_state: Pending }
  - Esc → 依 entry transition (Reader → ReaderScreen / Shelf → ShelfScreen 既有 ctor)
- [ ] draw / handle_event 內 polling JoinSet.try_join_next() append SearchResult
- [ ] UT INT-picker-spawn-01 / stream-02 / timeout-03 / enter-04 / enter-pending-05 / esc-shelf-06 / esc-reader-07 全綠（empty-shelf-08 屬 wire group、本 task 不驗）
- [ ] UT INT-picker-draw-01 全綠（ratatui TestBackend 跑 1 frame、驗 表格行數 + 各 row 顏色 — 對應 Seam 5）
- [ ] UT INT-picker-empty-source-list-09 全綠（0 enabled 書源 → spawn 0 task、表格永遠空、Enter no-op；advisory 文案「無 enabled 書源、請先 source import」可選實作）

### 步驟

#### 1. 新檔 + module decl
- [ ] 建 `src/presentation/handlers/tui/picker.rs`
- [ ] tui/mod.rs 加 `pub mod picker;`（不刪 switch_source 模組 — 那是 wire-03 的事）

#### 2. 資料模型
- [ ] 定義所有 enum + struct（按驗收標準列）
- [ ] use 既有 import: `tokio::task::JoinSet`、`tokio::time::timeout`、`async_trait`、`crossterm::event::{Event, KeyCode}`、`ratatui::widgets::ListState`、`catalog::SearchHit`、`crate::catalog::service::source::BookSource`、`crate::presentation::AppContext`

#### 3. SearchLike trait + Scraper impl
- [ ] `#[async_trait::async_trait(?Send)] pub(crate) trait SearchLike`
- [ ] `impl SearchLike for catalog::service::scraper::Scraper` forward 給既有 Scraper.search

#### 4. ctor + spawn_searches
- [ ] new() 初始化 phase=Picking、results=空 Vec、list_state=ListState::default()、join_set=None
- [ ] `spawn_searches<S: SearchLike + 'static + Clone>` 接 scraper handle 與 enabled sources Vec<BookSource>
  - 對每個 source 加 SearchResult { src_url, status: Loading, hit: None } 到 results
  - JoinSet spawn 一個 task per source、task 內：`tokio::time::timeout(5s, scraper.search(&src, &keyword)).await` 回 `(src_url, Status, Option<SearchHit>)`
  - 將 JoinSet move 進 self.join_set

#### 5. draw() Picking phase
- [ ] frame.area() 算 modal 中央 80% 寬 × 60% 高 + Clear widget 蓋背景
- [ ] 標題列「搜尋: <book_name> / <author> [N enabled 書源]」
- [ ] 表格列：書源 URL / 書名 / 作者 / 章數 / 狀態（用 ratatui Table）
- [ ] 提示列「j/k 移動 / Enter 選 / Esc 取消」
- [ ] ListState 同步 selected

#### 6. handle_event Picking phase
- [ ] poll join_set.try_join_next() (or 用 select_biased!) 收完成的 task 結果、更新 results 對應 row（按 src_url 找）
- [ ] j/k 移 list_state
- [ ] Enter：if selected row status == Ok → self.join_set.take().map(|mut j| j.abort_all())、phase = Confirming { selected_idx, sync_state: Pending }；else no-op
- [ ] Esc：dispatch by entry
  - Reader → Transition::To(Box::new(ReaderScreen::new(EntryMode::Direct, ctx, self.novel_id).await?))（從 DB 取既有 progress）
  - Shelf → Transition::To(Box::new(ShelfScreen::new()))

#### 7. UT
- [ ] INT-picker-spawn-01：mock SearchLike record call_count、enabled 3 sources、ctor + spawn_searches → call_count == 3
- [ ] INT-picker-stream-02：mock 3 sources 不同 delay (100ms/500ms/1000ms)、跑一輪 frame poll loop → results 順序依完成時間
- [ ] INT-picker-timeout-03：mock 1 source delay 6s → 5s 後 timeout、status = Timeout
- [ ] INT-picker-enter-04：mock 1 Ok source、Enter → phase = Confirming
- [ ] INT-picker-enter-pending-05：mock 0 Ok（全 Loading）、Enter → phase 仍 Picking
- [ ] INT-picker-esc-cancel-shelf-06：entry=Shelf、Esc → Transition::To 命中 ShelfScreen
- [ ] INT-picker-esc-cancel-reader-07：entry=Reader、Esc → Transition::To 命中 ReaderScreen
- [ ] INT-picker-draw-01：`TestBackend::new(80, 24)` + `Terminal::draw(|f| picker.draw(f, &ctx))` 跑 1 frame；用 backend.buffer 或 backend.test_buffer.assert_buffer 驗表格 row 數 == enabled 書源數 + 各 row 顏色（Loading/Timeout/Ok 對應色）
- [ ] INT-picker-empty-source-list-09：mock SearchLike + enabled sources=[]（空 Vec）→ spawn_searches call_count=0、results 永遠空；按 Enter no-op、phase 仍 Picking；draw 顯示 advisory 文案（若實作）

#### 8. baseline
- [ ] cargo build 無新 warning
- [ ] cargo test 既有 118 + 新 UT 全綠

---

## TASK-tui-picker-02: Confirming phase + sync_toc + fuzzy + atomic switch + caller-aware confirm Transition

**需求追溯**：REQ-002 / REQ-003 / REQ-004 / REQ-005
**目標**：Confirming phase 完整：進場後 async fetch_novel_info + fetch_toc + apply_fuzzy_filter with anchor + pick_best_with_anchor + threshold check + switch_source_core::run(target_idx)；caller-aware Transition 由 next_transition free fn 拆出（Seam 3）。

**驗收標準**：
- [ ] Confirming phase 進入時非同步起一個 sync_task（用 tokio::spawn 或 ctx 內呼）：
  - fetch_novel_info(new_src, hit.book_url)
  - fetch_toc → 若 toc.is_empty() → sync_state = Abort { EmptyToc }
  - apply_fuzzy_filter(old_chapter_name, &new_toc, Some(old_chapter_idx)) + pick_best_with_anchor → 若 best.score ≤ 50 → sync_state = Abort { FuzzyBelow(best.score) }
  - 否則 sync_state = Ok { new_idx, new_chapter_name, score }
- [ ] draw() Confirming phase：
  - Pending → 顯「準備換源、抓取新源 TOC 中...」
  - Ok → 顯「舊源第 {old+1} 章 《{old_name}》 → 新源第 {new+1} 章 《{new_name}》 (score {score})」+ 提示「y confirm / Esc 回」
  - Abort/Err → 顯訊息 + 「Esc 回」
- [ ] handle_event Confirming phase：
  - y（sync_state=Ok 時）→ switch_source_core::run(ctx, novel_id, new_src_url, new_book_url, target_idx=Some(new_idx)).await → 成功 Transition by entry / 失敗 sync_state = Err{ msg }
  - Esc → phase = Picking（不掉 results / join_set 已 None）
  - 其他鍵 nop
- [ ] `pub(crate) fn next_transition(entry: &PickerEntry, novel_id: i64, outcome: &SwitchOutcome) -> Transition` free fn（Seam 3）
- [ ] entry=Reader confirm 成功 → Transition::To(ReaderScreen::new(EntryMode::Direct, ctx, novel_id).await?)（從 DB 讀新 progress 即跳新章；不必新加 with_chapter ctor）
- [ ] entry=Shelf confirm 成功 → Transition::To(ShelfScreen::with_highlight_until(None, Some(toast_text), Instant::now() + TOAST_TTL))
- [ ] UT INT-transition-reader-confirm-01 / shelf-confirm-02 / esc-reader-03 / esc-shelf-04 + INT-fuzzy-threshold-01/02 + INT-edge-empty-toc-01 + INT-picker-confirming-pending-transition-09 全綠
- [ ] picker confirm 完成 transition reader 用既有 `ReaderScreen::new(EntryMode::Direct, ctx, novel_id).await?` ctor（不新加 with_chapter ctor、DB progress 已被 switch_source_tx 寫成新 target_idx、ctor 從 DB 讀即跳對章）

### 步驟

#### 1. Confirming phase 進場 async work
- [ ] 進入 Confirming { selected_idx, sync_state: Pending } 後在下一次 handle_event tick 跑 sync_task（或用 `tokio::task::spawn` + JoinHandle 儲存在 self.sync_join 欄位 + poll）
- [ ] 簡化選擇：Confirming 進場時直接 `async` 呼 fetch_novel_info → fetch_toc → fuzzy → 結果存 sync_state；UI 期間顯 Pending
- [ ] design 細節：picker.rs 內加 method `async fn perform_confirm_sync<S: SearchLike, D: SwitchSourceDeps>(...)`

#### 2. draw() Confirming phase
- [ ] match sync_state 渲染各狀態
- [ ] reuse Picking phase 的 Clear modal 框架

#### 3. handle_event Confirming phase
- [ ] y → 若 sync_state == Ok → switch_source_core::run with target_idx=Some(new_idx)
  - run Ok(outcome) → `next_transition(&self.entry, self.novel_id, &outcome)` 並 forward to runtime
  - run Err(AbortReason) → sync_state = Abort/Err 對應
- [ ] Esc → phase = Picking
- [ ] 其他鍵 → nop

#### 4. next_transition free fn (Seam 3)
- [ ] `pub(crate) fn next_transition(entry: &PickerEntry, novel_id: i64, outcome: &SwitchOutcome) -> Transition`
- [ ] match entry:
  - PickerEntry::Reader { .. } → 注意 ReaderScreen::new 是 async fn 需 ctx — Transition::To 接 Box<dyn Screen>、但建構需 await — 設計：在 caller 側 await new() 後再傳給 next_transition、或 next_transition 回 `enum NextScreen` 讓 caller awareness 處理
  - **修正設計**：next_transition 改回 `enum NextScreen { Reader, Shelf { toast: String } }`、caller 在 handle_event 內依此 enum 在 await 上下文 build screen → 包成 Transition::To。free fn 仍可 UT 直接驗 dispatch 邏輯
- [ ] UT 驗 enum 分派、不涉 ctx

#### 5. UT
- [ ] INT-transition-reader-confirm-01：entry=Reader{prev=10}、outcome.new_progress_idx=15 → next_transition 回 NextScreen::Reader
- [ ] INT-transition-shelf-confirm-02：entry=Shelf、outcome.new_progress_chapter_name="X"、new_progress_idx=15 → next_transition 回 NextScreen::Shelf{toast: "已換源 .. 目標：第 16 章 《X》"}
- [ ] INT-transition-esc-reader-03：handle_event Esc 在 Picking + entry=Reader → 走原 esc path 對應 NextScreen::Reader 等價（透過實際 handle_event flow + ratatui TestBackend 驗）
- [ ] INT-transition-esc-shelf-04：同上 entry=Shelf
- [ ] INT-fuzzy-threshold-01：mock fetch_toc + fuzzy 回 best score 50 → sync_state = Abort { FuzzyBelow(50) }、draw 顯訊息
- [ ] INT-fuzzy-threshold-02：best score 51 → sync_state = Ok、draw 顯預覽
- [ ] INT-edge-empty-toc-01：mock fetch_toc 回 empty Vec → sync_state = Abort { EmptyToc }、draw 顯「新源無章節」
- [ ] INT-picker-confirming-pending-transition-09：Enter 進 Confirming { sync_state: Pending } → 第 1 frame draw 顯「準備換源、抓取新源 TOC 中...」；mock sync_task 完成 → 下 1 frame draw 顯 sync_state 對應訊息（Ok / Abort）；驗 Pending → Ok/Abort state transition 真實發生

#### 6. baseline
- [ ] cargo build 無新 warning
- [ ] cargo test 全綠

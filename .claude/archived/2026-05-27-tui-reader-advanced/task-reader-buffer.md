# Tasks: reader-buffer
**前置群組**：infra（trait sig 必須先改）

> reader 核心架構改寫 — 從「single-chapter cursor」改成「Eager 3-chapter buffer + offset 表」。本 group 完成後 reader 視覺上可用、但只能用既有 j/k/Tab/Scroll 等鍵；TOC toggle、fuzzy、mouse 在後續 group 加。

---

## TASK-reader-buffer-01: ChapterBuffer 結構 + init_buffer / rebuild_buffer 兩支 API + scraper injection seam

**需求追溯**：REQ-006、design.md「Testability seams Seam 1」、「邊界處理 B1/B2」
**目標**：reader.rs 內定義 `ChapterBuffer` 結構與**兩支 fetch API**：`init_buffer`（reader 首次開啟、任一 fetch 失敗回 Err 整個 reader 開不起來）與 `rebuild_buffer`（reader 內跳章用、curr 失敗回 toast 不破壞舊 buffer）。同時建立 scraper injection seam 讓 UT 可 mock。

**驗收標準**：
- [ ] reader.rs 內有 `ChapterBuffer { combined_text, prev_chapter_idx, curr_chapter_idx, next_chapter_idx, prev_end_row, curr_end_row }` 結構
- [ ] **兩支 API 都實作**：
  - `async fn init_buffer(curr_idx, chapters, scraper_like, db) -> Result<ChapterBuffer>` — 任一章失敗 → Err
  - `async fn rebuild_buffer(curr_idx, chapters, scraper_like, db) -> Result<ChapterBuffer, RebuildError>` — `RebuildError::CurrFailed { idx, source }` vs `RebuildError::PartialDegraded(ChapterBuffer)`（prev/next 失敗回降級 buffer 而非 Err）
- [ ] 中段章節 → 3 章 buffer
- [ ] 第 0 章 → prev=None、buffer 2 章、prev_end_row = 0
- [ ] 最後章 → next=None、buffer 2 章
- [ ] **chapters 為空 → init_buffer 回 Err `anyhow!("無章節可讀")`**（B1 邊界）
- [ ] **章節 content 為空字串 → 視為 placeholder「（本章空白）」1 行**（B2 邊界）
- [ ] cache hit 不重 fetch；cache miss 呼 scraper + save_chapter_content 寫回
- [ ] **scraper injection seam**：透過 `ScraperLike` trait（pub(crate)，僅 `async fn fetch(&self, src, url) -> Result<String>`）讓 UT 注入 mock；production 用 wrapper 包 catalog::Scraper
- [ ] UT INT-buffer-01..06 + INT-boundary-empty-chapters + INT-boundary-empty-content 全綠

### 步驟

#### 1. 結構定義
- [ ] reader.rs 加 `struct ChapterBuffer { ... }`
- [ ] **加 `pub(crate) trait ScraperLike { async fn fetch_chapter_content(&self, src: &BookSource, url: &str) -> Result<String>; }`**
- [ ] production impl：`struct CrosstermScraperWrapper(catalog::Scraper)` 或直接 `impl ScraperLike for catalog::Scraper`
- [ ] reader 新增 `pub fn with_scraper_for_test<S: ScraperLike>(s: S, ...) -> Self` ctor（cfg-gated 或 cfg(test)）

#### 2. init_buffer / rebuild_buffer 兩支實作（含 B1/B2 邊界）
- [ ] init_buffer 邏輯：
  - 1) chapters.is_empty() → 立即 Err
  - 2) 算 prev_idx / next_idx（依 curr_idx 對 0 / last 邊界判斷）
  - 3) 各 idx 走 `library::facade::get_chapter` 取 cache；缺則 scraper.fetch + library::facade::save_chapter_content 寫回；fetch Err → 整個 init_buffer 回 Err
  - 4) 拼接 combined_text；對 content == "" 章節插入 placeholder「（本章空白）」
  - 5) 算 prev_end_row, curr_end_row
- [ ] rebuild_buffer 邏輯：
  - 1) 取 curr 章 cache or fetch；curr fetch Err → 立即 `RebuildError::CurrFailed { idx, source }`
  - 2) prev / next 各 fetch；任一 Err → 降級為更小 buffer 並 collect 失敗章
  - 3) 若 prev + next 都失敗 → 1 章 buffer + RebuildError::PartialDegraded(...) warning（不 Err，仍可讀 curr）
  - 4) 全成功 → Ok(buffer)

#### 3. fetch 策略（unknown #1）
- [ ] 先序列 fetch（cache hit 快、cache miss 少數）
- [ ] benchmark 完成後跑：開長篇 reader 平均 < 2 秒 → pass
- [ ] cache miss 多場景 > 5 秒考慮 tokio::join!；REQ-006 S1 已鬆綁「並行 vs 序列由實作決定」

#### 4. UT
- [ ] INT-buffer-01：mock 3 章 cache hit → buffer 3 章、offset 正確
- [ ] INT-buffer-02：curr=0 → prev=None、buffer 2 章
- [ ] INT-buffer-03：curr=last → next=None、buffer 2 章
- [ ] INT-buffer-04：3 章 cache hit → 注入 panic-on-call ScraperLike mock，呼 init_buffer → 不 panic（表示沒呼 scraper）
- [ ] INT-buffer-04b：cache miss 場景 → mock scraper return Ok("text") → init_buffer 成功 + chapters.content 從 NULL 變 "text"
- [ ] INT-buffer-05：mock scraper 對 next 章 Err → rebuild_buffer 回 `RebuildError::PartialDegraded`、buffer 仍含 prev + curr
- [ ] INT-buffer-06：mock scraper 對 curr 章 Err → rebuild_buffer 回 `RebuildError::CurrFailed`（caller TASK-reader-buffer-04 負責把這個轉成 toast）
- [ ] **INT-boundary-empty-chapters**：chapters = `[]` → init_buffer 立即 Err 帶訊息「無章節可讀」
- [ ] **INT-boundary-empty-content**：mock scraper 對 curr 章 return Ok("") → buffer.combined_text 含「（本章空白）」placeholder、prev_end_row / curr_end_row 計算正確

---

## TASK-reader-buffer-02: ReaderScreen state 重構（含全部後續 group 用的欄位 stub）

**需求追溯**：REQ-006、design.md「資料模型」
**目標**：ReaderScreen 結構從持「current chapter content」改持 ChapterBuffer；**同時把後續 group（toc / mouse）會用到的所有欄位一次 stub 進 struct**，避免後續 group 都動 struct 引發 merge 衝突。draw() 渲染 combined_text、scroll offset 是 buffer 內 row offset；既有 j/k 仍可用但語意改成「跳章 + buffer rebuild」。

**驗收標準**：
- [ ] ReaderScreen 加欄位（一次全加，stub 預設值）：
  - `buffer: ChapterBuffer`
  - `current: i64`（viewport top 章節）
  - `toc_list_state: ListState`
  - `toc_collapsed: bool` 預設 false（reader-toc-01 才 wire 行為）
  - `toc_width_cached: u16` 預設 0（reader-mouse-01 才 wire round-trip）
  - `mode: ReaderMode` 預設 `Normal`（reader-toc-02 才 wire 行為）
  - `toast: Option<String>` 預設 None
  - `toast_expires_at: Option<Instant>` 預設 None
- [ ] new() 用 `init_buffer` 而非 rebuild_buffer（init 失敗 → reader 開不起來）
- [ ] new() 內初始化 buffer = init_buffer(progress.chapter_index, ...)、scroll = prev_end_row
- [ ] draw() 顯示 buffer.combined_text、按 scroll offset 偏移 viewport
- [ ] draw() 同時更新 toc_width_cached（mouse-01 將 read 此欄位）
- [ ] j（normal mode）：current 移到下一章，rebuild buffer、scroll = prev_end_row of new buffer
- [ ] k：current 移上一章、同上
- [ ] J/K/Space/PgUp/Dn：scroll ± content_area_h（既有行為，但作用在 combined_text 上）

### 步驟

#### 1. struct 重構（一次到位）
- [ ] 移除既有的 `current_chapter_content: String` / `current_chapter_meta: ChapterMeta` 等欄位
- [ ] **加全部 8 個新欄位**（如驗收標準列）；reader-toc 與 reader-mouse 後續 group 不再動 struct 定義，只 wire 行為

#### 2. new() 改寫（用 init_buffer，B1 邊界）
- [ ] async fn new(entry_mode, ctx, novel_id) 內：
  - 1) 拉 chapters list
  - 2) chapters.is_empty() → 早回 Err（B1 邊界）
  - 3) 取 progress.chapter_index 為初始 curr
  - 4) buffer = init_buffer(curr, ...)；Err → reader 開不起來（上層帶 toast 顯示）
  - 5) scroll = buffer.prev_end_row（viewport 定位在 curr 開頭）

#### 3. draw() 改寫
- [ ] content pane 渲染 buffer.combined_text、按 scroll offset slice
- [ ] TOC pane 渲染 chapters list、highlight 在 current（用 toc_list_state）
- [ ] 底端進度條呼 `progress_text(buffer, scroll, total)` free fn
- [ ] **記 toc_width_cached** = if toc_collapsed { 0 } else { area.width * 30 / 100 }

#### 4. handle_event 既有鍵更新（用 rebuild_buffer 處理跳章失敗）
- [ ] j/k → current ± 1、呼 rebuild_buffer(new_curr)；CurrFailed → set toast、不改 state；Ok → scroll = new buffer prev_end_row + toc_list_state.select(Some(new_curr))
- [ ] J/K/Space/PgUp/Dn → scroll ± content_area_h、檢查邊界 → rebuild 或更新 progress
- [ ] **scroll 越界 + prev=None/next=None → 鎖在邊界、不再 rebuild**（B3 邊界）
- [ ] n/p → 同 j/k（保留既有 next/prev chapter 鍵）
- [ ] g/G → scroll = 0 or buffer 結尾
- [ ] Tab → focus 切換（保留）
- [ ] q/m → save progress 後 exit（保留）

#### 5. UT（純內部行為驗，不涉 mouse / fuzzy）
- [ ] INT-viewport-01/02/03：viewport_top_chapter 在三個區的回值（純 fn）
- [ ] INT-scroll-01：scroll 推到 next 結尾再 +1 → 觸發 rebuild、buffer 變 N+1±1
- [ ] **INT-jump-01a**：呼 reader 內部 `apply_jump_to(target_idx)` helper → rebuild_buffer、toc_list_state.selected()==Some(target_idx)、scroll = prev_end_row（INT-jump-01b 留 mouse-03 驗 click 入口）
- [ ] INT-jump-03：mock ScraperLike 對 curr 章 Err → rebuild_buffer 回 CurrFailed → reader.buffer / scroll / current 完全不變、reader.toast 帶錯誤訊息
- [ ] **INT-boundary-recursive-rebuild**：第 0 章 reader、scroll = 0、向上滾 → scroll 仍 0、無 rebuild、無 panic；對稱對最後章

---

## TASK-reader-buffer-03: viewport_top_chapter 與邊界 rebuild 觸發

**需求追溯**：REQ-006
**目標**：實作 `viewport_top_chapter(buffer, scroll)` 純函數；在 scroll 改變時自動偵測「越界 prev 開頭 / next 結尾」並 trigger rebuild_buffer。

**驗收標準**：
- [ ] viewport_top_chapter 純函數，按 design.md 邏輯實作
- [ ] handle_event scroll 操作後檢查邊界：scroll < 0 且有 prev → rebuild；scroll > combined_text.len() 且有 next → rebuild
- [ ] rebuild 觸發後 scroll 重算（讓視覺位置不跳）
- [ ] progress.chapter_index = viewport_top_chapter 每次 scroll 後更新（in-memory only，save 在 exit）
- [ ] UT INT-jump-01a / INT-scroll-01 + viewport-01/02/03 + INT-boundary-recursive-rebuild 全綠

### 步驟

- [ ] 加 helper `fn viewport_top_chapter(buffer: &ChapterBuffer, scroll: u16) -> i64`
- [ ] handle_event scroll 後加邊界檢查
- [ ] rebuild 觸發後 scroll = prev_end_row of new buffer（讓 viewport 對齊新 curr 開頭）
- [ ] UT 完整覆蓋

---

## TASK-reader-buffer-04: rebuild 失敗 toast wire + progress_text free fn

**需求追溯**：REQ-006 Scenario 8/9、design.md「Testability seams Seam 3」
**目標**：把 rebuild_buffer 回的 `RebuildError` 接成 reader.toast；progress_text 抽成 free fn 讓 INT-progress-01 直接驗。**API 拆分已在 buffer-01 完成（init + rebuild 兩支），本 task 不再拆**；toast 欄位已在 buffer-02 stub。

**驗收標準**：
- [ ] handle_event 中所有呼 rebuild_buffer 的點都 match `Result<Buffer, RebuildError>`：
  - Ok(buffer) → 換新 buffer
  - `Err(CurrFailed { idx, .. })` → reader.toast = "載入第 {idx+1} 章失敗"、TTL 3 秒、reader.buffer/scroll/current 不變
  - `Err(PartialDegraded(buffer))` → 用降級 buffer + toast「部分章節載入失敗、僅顯示可讀部分」
- [ ] `pub(crate) fn progress_text(buffer: &ChapterBuffer, scroll: u16, total: usize) -> String` 抽 free fn，回「第 X 章 / 共 N 章 (Y%)」
- [ ] draw() 底端用 progress_text() 渲染
- [ ] toast 渲染呼 toast_active()（reuse shelf-delete 的 pattern）
- [ ] UT INT-progress-01 全綠

### 步驟

- [ ] 加 `pub(crate) fn progress_text(buffer, scroll, total) -> String` free fn
- [ ] handle_event 鉤 RebuildError 三條分支（Ok / CurrFailed / PartialDegraded）
- [ ] draw() bottom bar 呼 progress_text
- [ ] UT INT-progress-01：呼 progress_text 用 mock buffer + scroll 在 prev/curr/next 三區 → 驗回字串正確

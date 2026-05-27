# Tasks: tui-wire
**前置群組**：tui-picker

> shelf 's' wire 改、reader 's' Normal mode wire、mod.rs 移除 switch_source、刪 switch_source.rs。三 task：shelf wire、reader wire、清理刪舊。

---

## TASK-tui-wire-01: shelf 's' wire 改為 SearchPickerScreen + 取 anchor 資料

**需求追溯**：REQ-005 / REQ-001（入口）
**目標**：shelf.rs 's' arm 從 SwitchSourceScreen 改為 SearchPickerScreen with PickerEntry::Shelf；取 highlight novel + progress + chapter name；既有 shelf 's' UT 改為驗 transition 到 SearchPickerScreen。

**驗收標準**：
- [ ] shelf.rs handle_event 's' arm 取 list_state.selected() → list_shelf[idx] novel_id / name / author
- [ ] 取 `library::facade::get_progress(novel_id)?.chapter_index`、取 `library::facade::get_chapter(novel_id, chapter_index)?.name`
- [ ] 若任一查不到（get_progress None / get_chapter None）→ Transition::To(ShelfScreen::with_highlight_until(None, Some("找不到舊章節，無法換源"), TTL))、不開 picker
- [ ] 成功取齊 → Transition::To(Box::new(SearchPickerScreen::new(PickerEntry::Shelf, novel_id, name, author, chapter_index, chapter_name)))
- [ ] spawn_searches 在 picker ctor 後立即執行（picker 內部 ctor 或 handle_event first tick）
- [ ] 空 shelf list_state.selected() == None → Transition::Stay（既有 d 鍵已有此 pattern）
- [ ] UT INT-entry-shelf-build-01 + INT-entry-shelf-empty-04 + INT-entry-shelf-chapter-not-found-05 全綠
- [ ] 既有 shelf 's' 鍵 UT（若有引用 SwitchSourceScreen 的）改為驗 SearchPickerScreen

### 步驟

#### 1. Read 既有 shelf 's' arm
- [ ] Read src/presentation/handlers/tui/shelf.rs:230-250 看現行 's' 處理
- [ ] Read library/facade.rs 看 get_progress / get_chapter 簽名

#### 2. 改 's' arm
- [ ] match selected → Some(idx) 取 novel + 查 progress + 查 chapter name
- [ ] 任一 Err 或 None → 帶 toast 留 shelf
- [ ] 成功 → 構造 SearchPickerScreen + Transition::To

#### 3. UT
- [ ] INT-entry-shelf-build-01：seed novels + progress + chapters、shelf list_state.select(Some(0))、按 s → transition 到 SearchPickerScreen、驗 picker.novel_id / book_name / old_chapter_idx / old_chapter_name 對齊
- [ ] INT-entry-shelf-empty-04：shelf 空、list_state.selected()=None、按 s → Transition::Stay
- [ ] INT-entry-shelf-chapter-not-found-05：seed shelf 有書、progress.chapter_index=99（chapters table 內無 idx=99 row）→ get_chapter 回 None → 不開 picker、Transition::To(ShelfScreen::with_highlight_until(.., Some("找不到舊章節，無法換源"), TTL))

#### 4. 既有 shelf 's' UT 重整
- [ ] grep shelf.rs::tests 找既有 's' 相關 UT
- [ ] 若 assert Transition::To(SwitchSourceScreen) 改為 SearchPickerScreen
- [ ] 若 assert 內容含 SwitchSourceScreen 特有資料、改用 PickerEntry::Shelf 對應

#### 5. baseline
- [ ] cargo build 無新 warning
- [ ] cargo test shelf 全綠

---

## TASK-tui-wire-02: reader Normal mode 's' wire + Filter mode 's' query 字元 verify

**需求追溯**：REQ-005 / REQ-001（入口）
**目標**：reader.rs handle_event Normal mode 加 `s` arm transition 到 SearchPickerScreen with PickerEntry::Reader{prev_chapter_idx}；Filter mode 既有 KeyCode::Char(c) 路徑已 append query — verify s 不誤觸 picker。

**驗收標準**：
- [ ] reader.rs handle_event Normal mode 加 `KeyCode::Char('s')` arm
- [ ] 取 reader.novel_id + reader.current (i64) + chapters[reader.current as usize].name
- [ ] 取 reader.book_name + reader.author（reader struct 已有？若無、從 chapters 第一個 ChapterMeta 反查 novel 表 — 看 reader.rs state）
  - 若 reader struct 沒 book_name/author，需從 `library::facade::get_novel(novel_id)` 取（單次 query、不阻塞）
- [ ] 成功 → Transition::To(Box::new(SearchPickerScreen::new(PickerEntry::Reader { previous_chapter_idx: reader.current }, novel_id, book_name, author, reader.current, chapter_name)))
- [ ] Filter mode 既有 `KeyCode::Char(c)` arm 不改（s 自動走 query append、不觸發 picker — verify by UT）
- [ ] UT INT-entry-reader-normal-build-02 + INT-entry-reader-filter-no-op-03 + INT-entry-reader-chapter-not-found-06 全綠
- [ ] book_name / author 取得策略明確：先 Read reader.rs ReaderScreen struct 看是否已有此兩欄位；若無、從 `library::facade::get_novel(novel_id)` 取（既有 fn），get_novel Err 視為「不可換源」、reader.toast 顯訊息

### 步驟

#### 1. Read 既有 reader 's' / Filter 處理
- [ ] Read reader.rs Normal mode handle_event 確認沒既有 's' arm
- [ ] Read reader.rs Filter mode KeyCode::Char(c) arm 確認 c='s' 走 append query

#### 2. 加 's' arm
- [ ] Normal mode 內 KeyCode::Char('s') match arm
- [ ] 取 novel_id（self.novel_id）+ current（self.current）+ chapters[current as usize].name
- [ ] 從 library::facade::get_novel 取 book_name + author（若 reader struct 沒這欄位）
- [ ] 構造 SearchPickerScreen + Transition::To
- [ ] mode guard：用 `if matches!(self.mode, ReaderMode::Normal)` 在 arm 條件式（同 't' 鍵既有 pattern）— **確認** reader 既有 handle_event 對 Filter mode 是否已分派路徑分離；若是，s 自動只進 Normal 分支不會撞 Filter（Filter 已 catch-all Char）

#### 3. UT
- [ ] INT-entry-reader-normal-build-02：seed reader Normal mode + chapters + novel、按 s → Transition::To(SearchPickerScreen) + entry == Reader{prev=current}
- [ ] INT-entry-reader-filter-no-op-03：reader Filter mode{query="入"}、按 s → reader.mode = Filter{query="入s"}、Transition::Stay、picker 不開
- [ ] INT-entry-reader-chapter-not-found-06：reader.current=99 但 chapters.len()=50 → 按 s → 不開 picker、reader.toast = "找不到舊章節，無法換源"、reader.toast_expires_at = Some(.. + TOAST_TTL)

#### 4. baseline
- [ ] cargo build 無新 warning
- [ ] cargo test reader + 全 suite 全綠

---

## TASK-tui-wire-03: 刪 switch_source.rs + mod.rs 清理 + CLI handler 確認

**需求追溯**：REQ-005（清理舊路徑）
**目標**：整片刪除 src/presentation/handlers/tui/switch_source.rs；tui/mod.rs 移除 `pub mod switch_source;`；確認沒其他檔還 import 它；確認 CLI handler `presentation/handlers/switch_source.rs`（CLI 入口、不是 tui）保留並驗證行為。

**驗收標準**：
- [ ] 刪 src/presentation/handlers/tui/switch_source.rs（包含其 tests mod）
- [ ] tui/mod.rs 移除 `pub mod switch_source;` 與相關 `pub use`
- [ ] grep `SwitchSourceScreen` 全 src/ 應 0 命中
- [ ] CLI handler src/presentation/handlers/switch_source.rs（非 tui/）保留、行為與本擴前等價（handler-core 已加 None 參數）
- [ ] cargo build clean（無 unused import warning）
- [ ] cargo test 全 118 baseline + 新 INT 全綠

### 步驟

#### 1. 刪檔
- [ ] `rm src/presentation/handlers/tui/switch_source.rs`
- [ ] Read tui/mod.rs 找 `pub mod switch_source` 行、刪
- [ ] Read tui/mod.rs 找 `pub use ... switch_source::*` 行（若有）、刪
- [ ] grep `SwitchSourceScreen` src/ → 0 命中

#### 2. 確認 import 清理
- [ ] grep `tui::switch_source` src/ → 0 命中
- [ ] grep `switch_source::SwitchSourceScreen` src/ → 0 命中
- [ ] grep `use ..switch_source.rs` 相關 → 0

#### 3. CLI 保留驗證
- [ ] Read presentation/handlers/switch_source.rs（CLI handler、不在 tui/ 下）確認未動
- [ ] CLI handler 應在 handler-core 階段已加 None 參數
- [ ] cargo run -- switch-source 1 https://... https://... 手動跑（或 INT 驗）行為與本擴前等價

#### 4. baseline 收尾
- [ ] cargo build 無新 warning（baseline 3 條 dead_code 維持）
- [ ] cargo test 全綠（118 + 新 INT 30+）
- [ ] 整批 spec 驗收：8 criteria 全部 backed

# Tasks: infra
**前置群組**：無（最先做；同時 commit Cargo.toml dep 與 Screen trait 改動）

**前置條件**：shelf-delete 必須已 merge 到 main（precondition gate 見 TASK-infra-01 步驟 0）

> 本 group 的 TASK-infra-01 是 big-bang change：trait sig 變動會讓 6 個既有 screen 同時編譯失敗，必須在同一 task 內全數修補完畢。中間狀態 cargo build 會紅。

---

## TASK-infra-01: Screen trait 改為 Event + 6 screen 配合 + run_loop forward Mouse

**需求追溯**：REQ-001
**目標**：`Screen::handle_event` 簽名從 `KeyEvent` 改為 `Event`；run_loop 不再吞 MouseEvent；6 個既有 screen 全配合改造後 `cargo build` + `cargo test` 全綠、既有行為斷言不變。

**驗收標準**：
- [ ] `tui/mod.rs` 的 `Screen` trait `handle_event` 第 1 個參數型別為 `crossterm::event::Event`
- [ ] `tui/mod.rs` 的 `run_loop` 接受 `Event::Key | Event::Mouse`、其他 event type 走 `continue`
- [ ] 6 個既有 screen（menu / shelf / reader / search / switch_source / delete_confirm）的 `handle_event` 簽名更新、`match event` 第一層解構 Key / Mouse
- [ ] 6 個 screen 對 `Event::Mouse(_)` 都返回 `Transition::Stay`（reader 本 task 暫不接 mouse，後續 reader-mouse group 處理）
- [ ] 全 cargo test 通過（既有 UT 都改為包 `Event::Key(KeyEvent::new(...))`、行為斷言完全一致）
- [ ] 新增 UT `int_trait_01_menu_mouse_stay`、`int_trait_02_shelf_mouse_stay`、`int_trait_03_delete_confirm_mouse_stay`、`int_trait_04_run_loop_forwards_mouse`、`int_trait_05_run_loop_ignores_resize_paste_focus`

### 步驟

#### 0. Precondition gate（必先確認）
- [ ] 跑 `git log main..HEAD -- src/presentation/handlers/tui/delete_confirm.rs` 確認 delete_confirm.rs 已在 main 之上（shelf-delete 已 merge）；若空輸出表示 shelf-delete 未 merge，**停下不開工**
- [ ] 跑 `pwd` 確認在 main-based 新 worktree（如 `dev/2026-05-27-tui-reader-advanced`），非 shelf-delete worktree

#### 1. trait 簽名 + run_loop 重構（testability seam）
- [ ] 改 `tui/mod.rs::Screen::handle_event` 簽名 KeyEvent → Event
- [ ] **重構 run_loop 拆 testable inner fn**（見 design.md「Testability seams Seam 2」）：
  - 保留 `pub async fn run_loop(app: App) -> Result<()>` 作 thin wrapper、內部呼 `run_inner`
  - 新加 `async fn run_inner<E: EventSource, T: TerminalLike>(app: App, events: &mut E, term: &mut T) -> Result<()>`
  - 新加 `pub(crate) trait EventSource { fn poll(&mut self, dur: Duration) -> Result<bool>; fn read(&mut self) -> Result<Event>; }`
  - production 用 `struct CrosstermEventSource` 包 `crossterm::event::poll` / `read`
- [ ] `run_inner` event match：`Event::Key | Event::Mouse` → forward；其他 type → `continue`
- [ ] 確認 `KeyEventKind::Press` filter 仍對 Key event 生效（Mouse event 不過此 filter）

#### 2. 6 screen 配合改 sig（不變行為）
- [ ] menu.rs：handle_event sig 更新；match event → Event::Key(key) 走原邏輯、Event::Mouse(_) → Stay
- [ ] shelf.rs：同上（保留 shelf-delete 加的 d/s + toast TTL clear 邏輯）
- [ ] reader.rs：同上（保留所有既有 binding，mouse 留待 reader-mouse group）
- [ ] search.rs：同上
- [ ] switch_source.rs：同上
- [ ] delete_confirm.rs：同上（shelf-delete 新增的 screen）

#### 3. 既有 UT 更新
- [ ] 全 grep `handle_event(KeyEvent::new(...)` → 改為 `handle_event(Event::Key(KeyEvent::new(...)), ...)`
- [ ] 跑 `cargo test` 確認既有 UT 全綠（這是 trait migration 不退化的 ground truth）

#### 4. 新增 trait-level UT
- [ ] `int_trait_01_menu_mouse_stay`：給 menu 餵 Event::Mouse(ScrollUp) → Transition::Stay
- [ ] `int_trait_02_shelf_mouse_stay`：給 shelf 餵 Event::Mouse(Down(Left)) → Transition::Stay
- [ ] `int_trait_03_delete_confirm_mouse_stay`：給 delete_confirm 餵 Event::Mouse(ScrollUp) → Transition::Stay
- [ ] `int_trait_04_run_loop_forwards_mouse`：mock `EventSource` queue 含 Mouse 事件 + 結束 sentinel；mock Screen 記錄 received events → 呼 `run_inner(...)` 一輪 → 驗 Screen 收到 Mouse event
- [ ] `int_trait_05_run_loop_ignores_resize_paste_focus`：mock `EventSource` queue 含 Resize / Paste / FocusGained + 結束 sentinel；mock Screen 記錄 received events → 呼 `run_inner` → 驗 Screen 沒收到這三類（loop 走 continue）、不 panic

#### 5. 整體驗證
- [ ] `cargo build`：無 warning（除原本就有的 3 條 dead_code）
- [ ] `cargo test`：全綠

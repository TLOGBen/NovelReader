# Goal

## 目標（Goal）

在 TUI Reader 加入無縫跨章節滾動（Eager 三章 buffer）、可 toggle 的 TOC 側欄、fuzzy filter、滑鼠滾輪與點擊支援，並把 Screen trait 從 `KeyEvent` 統一到 `Event` 介面；完成後使用者在 reader 中讀長篇小說時，跨章節無視覺斷裂、可用 `/` 快速跳章、可用滑鼠操作、TOC 可縮合留更多閱讀空間。

## 驗收標準（Criteria）

- [ ] **C1：無縫跨章** — 在 reader 內滾動到當前章節尾，繼續滾動立刻進入下一章開頭、視覺上是同一個連續長文，無「載入中」畫面或可見斷裂
- [ ] **C2：TOC toggle** — 按 `t` 鍵可在 TOC 側欄寬度 30% ↔ 0% 兩態切換
- [ ] **C3：Fuzzy filter** — 按 `/` 進入 filter mode，邊打邊即時 filter TOC list；Enter 跳到當前選中的章；Esc 取消回完整 TOC
- [ ] **C4：滑鼠滾輪** — content pane 滾輪一次滾 3 行；TOC pane 滾輪一次移動 1 章
- [ ] **C5：點擊跳章** — 單擊 TOC 列直接跳到該章；content pane 點擊為 no-op
- [ ] **C6：Trait migration** — `Screen` trait 統一為 `handle_event(Event, &mut AppContext) -> Transition`，6 個既有 screen（menu / shelf / reader / search / switch_source / delete_confirm）全配合，既有 UT 全綠
- [ ] **C7：cargo test 全綠** — 既有測試 + 新增 UT 全通過
- [ ] **C8：手動驗收** — `cargo install --path . --force` 後手動跑 reader，C1-C5 五項視覺/操作上可確認

## 範圍（Scope）

### 包含（In scope）

- `tui/mod.rs` Screen trait 簽名改為 `handle_event(Event, ...)`、run_loop forward MouseEvent + KeyEvent
- 6 個既有 screen 配合 trait migration（含 UT）
- `reader.rs` Eager 三章 buffer（核心架構改寫，state 持 prev/curr/next 三章 + 各章 offset 表）
- `reader.rs` TOC toggle（`t` 鍵、Layout constraints runtime mutation）
- `reader.rs` Fuzzy filter mode（`/` 進入、邊打邊 filter、Enter / Esc 出）
- `reader.rs` Mouse wheel hit-test（pane-aware speed）
- `reader.rs` Mouse click on TOC（hit-test → chapter idx）
- `Cargo.toml` 加 `fuzzy-matcher = "0.3"` dep

### 不包含（Out of scope）

- **無限滾動 > 3 章累積** — buffer 永遠 3 章，超過邊界 trigger rebuild；避免記憶體無限增長
- **章節名 parse 卷/部做縮排** — 不同網站書源寫法差太多，脆弱
- **內文 typography 美化**（字寬限制 / 段距 / 配色 / X/N 文字進度條的視覺設計細節）— 用 TOC toggle + 無縫切章解使用者真正的痛點，非追求字面美化
- **search screen 也採 fuzzy** — 本輪只動 reader TOC，search 維持既有邏輯
- **Mouse hover 高亮** — 僅實作 click + scroll，hover 屬下一輪 polish
- **Persistent 底端 search bar** — `/` 是 modal-style，不是常駐 bar
- **resize / paste / focus event 處理** — Screen trait 改 Event 後免費獲得入口，但本輪不主動處理（屬下一輪）
- **跨 session reading buffer 持久化** — 重開 reader 從 `progress.chapter_index` 重建 buffer，不存 buffer state

# Requirements

## REQ-001: Screen trait 統一 Event 介面

**描述**：`Screen` trait 簽名從 `handle_event(KeyEvent, &mut AppContext) -> Transition` 改為 `handle_event(Event, &mut AppContext) -> Transition`；run_loop forward 所有 crossterm `Event::Key` 與 `Event::Mouse`；6 個既有 screen 全配合改造，未實際使用的 event type 走 `Transition::Stay`。

### Scenarios

**Scenario 1：既有 keyboard 行為不退化**
- **Given** 使用者開啟 TUI 主菜單
- **When** 按 `j`、`k`、`Enter`、`q` 等既有 binding
- **Then** 行為與 trait migration 前完全一致（既有 UT 全綠）

**Scenario 2：MouseEvent 傳達到當前 screen**
- **Given** TUI 在任一 screen 上
- **When** crossterm 產生 `Event::Mouse(...)`
- **Then** run_loop 不再 `else { continue }` 吃掉，而是 forward 給 `current.handle_event(Event::Mouse(...), ...)`
- **And** 不處理 mouse 的 screen 返回 `Transition::Stay`，不報錯

**Scenario 3：UT 跟著 sig 改但行為斷言不變**
- **Given** 6 個既有 screen 各有 UT 呼叫 `handle_event(KeyEvent::new(...), ...)`
- **When** trait sig 改為 Event
- **Then** UT 改成 `handle_event(Event::Key(KeyEvent::new(...)), ...)`，行為斷言（Transition 結果、state mutation）完全不變

---

## REQ-002: 可 toggle 的 TOC 側欄

**描述**：reader 內按 `t` 鍵在 TOC pane 寬度 30% ↔ 0% 兩態切換，提供「縮合 sidebar 留更多閱讀空間」與「展開 sidebar 看 TOC」兩個狀態。

### Scenarios

**Scenario 1：初始展開**
- **Given** 使用者從 menu / shelf 進入 reader
- **When** reader 首次渲染
- **Then** TOC pane 寬度為 area.width 的 30%、content pane 占 70%

**Scenario 2：按 t 縮合**
- **Given** reader TOC pane 寬度為 30%
- **When** 按 `t` 鍵
- **Then** TOC pane 寬度變為 0%（不渲染）、content pane 占 100%
- **And** content pane 既有 scroll offset 不受影響

**Scenario 3：再按 t 展開**
- **Given** reader TOC pane 寬度為 0%
- **When** 按 `t` 鍵
- **Then** TOC pane 寬度恢復為 30%、content pane 縮回 70%

**Scenario 4：filter mode 期間 t 鍵正常**
- **Given** reader 處於 fuzzy filter mode（`/` 已按）
- **When** 在 input bar 輸入 `t`
- **Then** `t` 是 filter query 的一部分（append 到 query），**不**觸發 TOC toggle
- **And** 退出 filter mode 後 `t` 鍵恢復 toggle 語意

---

## REQ-003: Fuzzy filter mode

**描述**：reader 內按 `/` 進入 fuzzy filter input mode，邊打邊即時 filter TOC 的章節列表；Enter 跳到當前選中的章；Esc 取消、回完整 TOC。使用 `fuzzy-matcher` crate 的 `SkimMatcherV2`。

### Scenarios

**Scenario 1：進入 filter mode**
- **Given** reader 處於 normal mode（既有 j/k/Tab 等 binding 有效）
- **When** 按 `/` 鍵
- **Then** TOC pane 底端顯示 input bar、游標放在 input bar
- **And** TOC list 仍顯示全部章節（query 為空時不 filter）
- **And** state 切到 filter mode（normal mode 的 j/k 暫停接管）

**Scenario 2：邊打邊 filter**
- **Given** 處於 filter mode、input bar 為空
- **When** 輸入字元 `入魔`
- **Then** TOC list 即時收縮為 fuzzy 命中「入魔」的章節（按 SkimMatcherV2 分數降序）
- **And** highlight 預設指向第一個結果

**Scenario 3：filter mode 中 j/k 移動**
- **Given** filter 結果有 5 個章節、highlight 在第 1 個
- **When** 按 `j`（一般 mode 用來移章節，filter mode 改成移 filtered list）
- **Then** highlight 移到 filtered list 第 2 個
- **And** 不影響 normal mode 的 chapters 全列表

**Scenario 4：Backspace 刪 query**
- **Given** input bar 是「入魔」
- **When** 按 `Backspace`
- **Then** query 變成「入魔」少一字元、filter 結果重算
- **And** highlight 若指向的章已被 filter 掉，重設到 filtered list 第 1 個

**Scenario 5：Enter 跳到選中章**
- **Given** filter 結果有命中、highlight 在某章
- **When** 按 `Enter`
- **Then** 跳到該章（rebuild 三章 buffer、scroll 跳到該章開頭）
- **And** 退出 filter mode、回 normal mode
- **And** filter query 清空

**Scenario 6：Esc 取消**
- **Given** 處於 filter mode（不論 query 為空或有內容）
- **When** 按 `Esc`
- **Then** 退出 filter mode、回 normal mode
- **And** query 清空、TOC 顯示全部章節
- **And** 不改變當前讀的章節（沒有跳章）

**Scenario 7：filter 結果為空**
- **Given** filter mode、輸入查不到任何章節（如「xyzqwerty」）
- **When** 按 `Enter`
- **Then** 不跳章、不報錯、不退出 filter mode（讓使用者繼續修改 query）

**Scenario 8：filter 進入時 TOC 強制展開、退出不還原**
- **Given** reader 處於 normal mode、TOC 為收合狀態（toc_collapsed = true）
- **When** 按 `/` 進入 filter mode
- **Then** toc_collapsed 強制設為 false（TOC 必須可見才能 filter）
- **And** 退出 filter mode（Esc / Enter）後 toc_collapsed 不還原（保持 false，使用者若想收回得自行按 `t`）

---

## REQ-004: 滑鼠滾輪支援

**描述**：滑鼠滾輪 ScrollUp / ScrollDown 在 reader 的不同 pane 有不同 step：content pane 一次滾 3 行；TOC pane 一次移動 1 章。Hit-test 用 MouseEvent 的 column 判 pane。

### Scenarios

**Scenario 1：content pane 滾輪向下**
- **Given** reader 在 normal mode、TOC 展開（30%/70%）、滑鼠在 content pane（column > TOC width）
- **When** 滾輪向下滾 1 格
- **Then** scroll offset 增加 3 行
- **And** focus 不變（不切到 TOC pane）

**Scenario 2：content pane 滾輪向上**
- **Given** 同上
- **When** 滾輪向上滾 1 格
- **Then** scroll offset 減少 3 行（最小 0）

**Scenario 3：TOC pane 滾輪向下**
- **Given** 滑鼠在 TOC pane（column < TOC width 且 TOC 展開）
- **When** 滾輪向下
- **Then** TOC highlight 下移 1 章（觸發跳章、buffer rebuild）

**Scenario 4：TOC pane 滾輪向上**
- **Given** 同上
- **When** 滾輪向上
- **Then** TOC highlight 上移 1 章（觸發跳章）

**Scenario 5：TOC 收合時整個畫面當 content pane**
- **Given** TOC 已 toggle 為 0%
- **When** 任何 column 滾輪
- **Then** 走 content pane 行為（滾 3 行）

**Scenario 6：filter mode 期間滾輪**
- **Given** filter mode、滑鼠在 TOC pane
- **When** 滾輪
- **Then** highlight 在 filtered list 中移動 1 個（不在全列表）

---

## REQ-005: 點擊 TOC 跳章

**描述**：滑鼠單擊 TOC pane 任一列直接跳到該章；content pane 點擊為 no-op。

### Scenarios

**Scenario 1：點擊 TOC 跳章**
- **Given** TOC 展開、列表顯示 chapter 0..50、滑鼠在 TOC pane row 5（對應 chapter 5）
- **When** `MouseEvent::Down(MouseButton::Left)`
- **Then** highlight 移到 chapter 5、跳章（rebuild 三章 buffer、scroll 到 chapter 5 開頭）

**Scenario 2：content pane 點擊**
- **Given** 滑鼠在 content pane
- **When** 左鍵點擊
- **Then** 無事發生（不跳章、不改 scroll、不報錯）

**Scenario 3：TOC pane 點擊空白區（list 短於畫面）**
- **Given** TOC 只有 3 章，畫面剩餘 row 為空白
- **When** 點空白 row
- **Then** 無事發生（hit-test 命中為 None）

**Scenario 4：filter mode 期間點 TOC**
- **Given** filter mode、filtered list 有 5 章
- **When** 點 filtered list row 2
- **Then** 跳到該章、退出 filter mode（同 Enter 行為）

---

## REQ-006: Eager 三章 buffer（無縫切章）

**描述**：reader 內文採 Eager 三章 buffer 模型，state 持 prev + current + next 三章 content 拼成單一連續 String，附各章在 buffer 中的 offset 表；scroll 跨章邊界無視覺斷裂；越過 prev 開頭或 next 結尾自動 rebuild buffer（fetch + 重組）。第 0 章與最後章降級 2 章 buffer。

### Scenarios

**Scenario 1：開啟 reader buffer init**
- **Given** 使用者從 menu/shelf 進入 chapter N 的 reader
- **When** reader 首次 init
- **Then** fetch chapter N-1、N、N+1 的 content（cache hit 不重 fetch；fetch 策略 — 並行 vs 序列 — 屬實作細節，本 Then 只斷言三章 content 取得成功）
- **And** 拼成 buffer = `prev_content + "\n\n" + curr_content + "\n\n" + next_content`
- **And** scroll offset 設為 prev_content 長度（讓 viewport 預設定位在 chapter N 開頭）
- **And** offset 表記住 prev/curr/next 在 buffer 中的起始 row

**Scenario 2：第 0 章 buffer 降級**
- **Given** 使用者開啟 chapter 0 的 reader
- **When** buffer init
- **Then** 只 fetch chapter 0 + chapter 1（無 prev）
- **And** buffer = `curr_content + "\n\n" + next_content`、scroll offset = 0

**Scenario 3：最後章 buffer 降級**
- **Given** 使用者開啟最後一章 N（chapters.len() - 1）
- **When** buffer init
- **Then** 只 fetch chapter N-1 + chapter N（無 next）
- **And** buffer = `prev_content + "\n\n" + curr_content`、scroll offset = prev_content 長度

**Scenario 4：跨章邊界滾動無斷裂**
- **Given** buffer 有 prev+curr+next、scroll 在 curr 中段
- **When** 持續向下滾動超過 curr 結束 row
- **Then** viewport 顯示的內容立刻接到 next 開頭的文字、視覺上不暫停、不出現 loading
- **And** 進度條 X 從 N 跳變為 N+1（以 viewport top 那行所屬章為準）

**Scenario 5：滾過 next 結尾觸發 rebuild**
- **Given** buffer = N-1 / N / N+1、scroll 推進到 next（N+1）結尾
- **When** 繼續向下滾動 1 行
- **Then** 觸發 buffer rebuild：fetch N+2、buffer 變成 N / N+1 / N+2
- **And** scroll offset 重算（保持當前可視內容相對位置不變）
- **And** progress.chapter_index 更新為 N+1（viewport top）

**Scenario 6：滾過 prev 開頭觸發 rebuild**
- **Given** buffer = N-1 / N / N+1、scroll 推進到 prev（N-1）開頭以上
- **When** 繼續向上滾動 1 行
- **Then** 觸發 buffer rebuild：fetch N-2、buffer 變成 N-2 / N-1 / N
- **And** scroll offset 重算
- **And** progress.chapter_index 更新為 N-1

**Scenario 7：點 TOC 跳章完整 rebuild**
- **Given** buffer 是任何狀態
- **When** 點 TOC chapter M（或 fuzzy filter Enter）
- **Then** buffer 完全 rebuild 為 M-1 / M / M+1（含降級）
- **And** scroll offset 設為 prev 長度（viewport 在 M 開頭）
- **And** progress.chapter_index = M

**Scenario 8：rebuild 期間 fetch 失敗**
- **Given** rebuild 觸發但 next chapter fetch 失敗（網路 / 書源錯誤）
- **When** fetch 返回 Err
- **Then** buffer 不變、scroll 不變、提示 toast「下一章載入失敗」
- **And** 不 panic、不破壞當前 buffer

**Scenario 9：進度條以 viewport top 為準**
- **Given** buffer 跨章、viewport 顯示 N 末尾 + N+1 開頭
- **When** 渲染進度條
- **Then** X = N（viewport top 那行所屬章），不是 N+1

---

說明：

- REQ-001 是 infra change，被 REQ-002~006 在 trait migration 前置條件上依賴
- REQ-002（toggle）獨立、與其他 REQ 無耦合
- REQ-003（fuzzy）會跟 REQ-005（click）共用「跳章」邏輯
- REQ-004（wheel）與 REQ-005（click）共用 hit-test 函數
- REQ-006（buffer）是被 REQ-003 / REQ-005 觸發的「跳章」動作的接收端，也是 reader 核心架構

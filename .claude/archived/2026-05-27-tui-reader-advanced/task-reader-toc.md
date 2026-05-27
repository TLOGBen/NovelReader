# Tasks: reader-toc
**前置群組**：infra（trait sig）+ reader-buffer（跳章邏輯 + buffer rebuild API + ReaderScreen struct 已 stub mode / toc_collapsed 欄位）

> TOC toggle + fuzzy filter mode；本 group 只 wire 既有 stub 欄位的行為（不動 struct 定義）。REQ-002 S4 + REQ-003 S8（filter 進入強制展開 TOC）相關行為由 INT-mode-04 覆蓋。

---

## TASK-reader-toc-01: TOC toggle (t 鍵)

**需求追溯**：REQ-002
**目標**：reader 加 `toc_collapsed: bool` 欄位、按 `t` 切換；draw() 按 toc_collapsed 動態算 Layout constraints；filter mode 期間 't' 走 query append 不 toggle。

**驗收標準**：
- [ ] ReaderScreen 加 `toc_collapsed: bool`，new() 預設 false
- [ ] handle_event Event::Key(KeyEvent{ code: Char('t'), ... }) in Normal mode → toc_collapsed = !toc_collapsed
- [ ] draw() 計算 toc_width = if toc_collapsed { 0 } else { area.width * 30 / 100 }
- [ ] Layout constraints 動態反映：toc_collapsed=true → content pane 占 100%，TOC pane 不渲染
- [ ] filter mode 期間 't' 走 append query（reader-toc-02 task 處理 filter mode；本 task 只確保 normal mode toggle）
- [ ] UT INT-toggle-01：t 兩次回原狀

### 步驟

- [ ] 加欄位
- [ ] handle_event 加 Char('t') arm（mode == Normal 才 toggle）
- [ ] draw() 改用 toc_width 計算 Layout
- [ ] UT INT-toggle-01

---

## TASK-reader-toc-02: Fuzzy filter mode state + dep

**需求追溯**：REQ-003
**目標**：加 `ReaderMode { Normal, Filter { query, filtered_indices, selected } }`、Cargo.toml 加 fuzzy-matcher dep；按 `/` 進 filter mode、輸入字元 append query 並重算 filter、Backspace 刪字元、Esc 取消、Enter 跳到 selected 章。

**驗收標準**：
- [ ] Cargo.toml 加 `fuzzy-matcher = "0.3"`
- [ ] reader.rs 定義 `enum ReaderMode { Normal, Filter { ... } }`、reader struct 加 `mode: ReaderMode`
- [ ] handle_event 在 Normal mode 接 Char('/') → mode = Filter { query: "", filtered_indices: <all idx>, selected: 0 }；toc_collapsed 強制設 false（filter 需顯 TOC）
- [ ] Filter mode 接 Char(c) c != Esc / Enter → query += c、filtered_indices 用 SkimMatcherV2 重算（chapters.name 對 query 跑 fuzzy_match、取 Some 結果按 score 降序、collect indices）、selected = 0
- [ ] Filter mode 接 Backspace → query.pop()、重算 filter、selected = 0
- [ ] Filter mode 接 Esc → mode = Normal、清 query / filtered_indices
- [ ] Filter mode 接 Enter → 跳到 chapters[filtered_indices[selected]]（rebuild_buffer + scroll）、mode = Normal、清 query
- [ ] Filter mode 接 j/k → selected ± 1 in filtered_indices（不出界）
- [ ] Filter mode + 空 filtered_indices + Enter → 不跳章不退出（用戶繼續修改 query）
- [ ] UT INT-mode-01/02/03 全綠

### 步驟

#### 1. dep
- [ ] Cargo.toml [dependencies] 加 `fuzzy-matcher = "0.3"`
- [ ] reader.rs use `fuzzy_matcher::skim::SkimMatcherV2`、`fuzzy_matcher::FuzzyMatcher`

#### 2. struct + mode enum
- [ ] 加 `enum ReaderMode { Normal, Filter { query: String, filtered_indices: Vec<usize>, selected: usize } }`
- [ ] reader struct 加 `mode: ReaderMode`，new() 設 Normal

#### 3. fuzzy helper
- [ ] `fn apply_fuzzy_filter(query: &str, chapters: &[ChapterMeta]) -> Vec<usize>`
- [ ] 邏輯：SkimMatcherV2::default()、iter chapters 跑 fuzzy_match(name, query)、filter_map Some(score) → collect (idx, score)、sort_by score desc、map idx
- [ ] query 為空時回 (0..chapters.len()).collect()

#### 4. handle_event 改造
- [ ] Normal mode 接 '/' → 切到 Filter、強制 toc_collapsed = false
- [ ] Filter mode 處理 Char / Backspace / Esc / Enter / j / k 如上述驗收標準

#### 5. draw() 配合
- [ ] Filter mode 渲染：TOC pane 底端加 input bar 顯示 "/" + query
- [ ] TOC list 顯示 chapters[filtered_indices[..]]、highlight 在 filtered_indices[selected]
- [ ] Normal mode draw 不變

#### 6. UT
- [ ] INT-mode-01：state transition Normal → Filter → Normal
- [ ] INT-mode-02：query="入魔" 從 3 章 list filter 命中對應的
- [ ] INT-mode-03：CJK 字元 SkimMatcherV2 命中（"123" → "第123章"、"入魔" → "...入魔之路"）
- [ ] INT-mode-04：query="" Backspace no panic；toc_collapsed=true 按 / → 強制 toc_collapsed=false；Esc 後仍 false
- [ ] INT-toggle-02：mode=Filter、按 't' → query += 't'、toc_collapsed 不變

---

## TASK-reader-toc-03: Filter mode UX 邊界

**需求追溯**：REQ-003 Scenario 4/6/7
**目標**：Backspace 在 query 空時不報錯；Esc 不影響當前讀的章；filtered_indices 空時 Enter 不跳不退出。

**驗收標準**：
- [ ] Backspace + query 空 → no-op、不 panic
- [ ] Esc 退出 filter 後 reader buffer + scroll + current 不變
- [ ] filtered_indices 空 + Enter → no-op、mode 仍 Filter
- [ ] inline UT 覆蓋

### 步驟

- [ ] 三個 edge case 加 UT、必要時補防呆

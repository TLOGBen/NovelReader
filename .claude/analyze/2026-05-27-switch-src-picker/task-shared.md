# Tasks: shared
**前置群組**：無

> apply_fuzzy_filter 簽名擴成 cross-book mapping API。改 reader.rs 內部 1 處簽名 + 5 處 caller cascade（2 production + 3 test asserts）+ 新加 helper free fn。reader Filter mode 5 個既有 UT 完全不退化。

---

## TASK-shared-01: apply_fuzzy_filter 簽名擴 + 5 caller cascade + pick_best_with_anchor helper

**需求追溯**：REQ-002 / REQ-006
**目標**：reader.rs apply_fuzzy_filter 簽名從 `(query, chapters) -> Vec<usize>` 改為 `(query, chapters, anchor: Option<i64>) -> Vec<(usize, i64)>`、回 `(idx, score)` 對；5 caller 全 wrap 不退化；加 pick_best_with_anchor helper 供 picker confirm phase 用。

**驗收標準**：
- [ ] `apply_fuzzy_filter` 新簽名 `(query: &str, chapters: &[ChapterMeta], anchor: Option<i64>) -> Vec<(usize, i64)>`
- [ ] 排序：primary `score desc`、secondary `|idx as i64 - anchor.unwrap_or(idx as i64)| asc`（anchor=None 走 stable order）
- [ ] reader.rs:862（reader-toc-02 handle_filter_key 的 query append 路徑）caller wrap `.into_iter().map(|(i,_)| i).collect()`
- [ ] reader.rs:902（同上 Backspace 路徑）caller wrap 同上
- [ ] reader.rs:1938（INT-mode-02 `assert_eq!(result, vec![1])`）改 wrap
- [ ] reader.rs:1957（INT-mode-03 `r1.contains(&0)`）改 wrap
- [ ] reader.rs:1959（INT-mode-03 `r2.contains(&1)`）改 wrap
- [ ] 新加 `pub(crate) fn pick_best_with_anchor(scored: &[(usize, i64)], anchor: Option<i64>) -> Option<(usize, i64)>` free fn — primary score desc / secondary `|i as i64 - anchor| asc` / 全空 → None
- [ ] UT INT-fuzzy-anchor-01（tiebreak proximity）+ INT-fuzzy-anchor-02（None stable order）+ INT-fuzzy-cjk-mapping-01（跨書源中文章名）+ INT-fuzzy-cascade-reader-01/02（既有 5 UT regression）全綠

### 步驟

#### 1. 簽名擴
- [ ] Read reader.rs:364-376 看 apply_fuzzy_filter 既有實作
- [ ] 改簽名為 `(query: &str, chapters: &[ChapterMeta], anchor: Option<i64>) -> Vec<(usize, i64)>`
- [ ] 內部排序保留既有 SkimMatcherV2 + score desc；anchor=Some(a) 時加 secondary key tuple `(score, -|i as i64 - a|)` 排序

#### 2. 5 caller cascade
- [ ] grep `apply_fuzzy_filter(` 確認共 5 處
- [ ] reader.rs:862 / :902 production caller wrap
- [ ] reader.rs:1938 / :1957 / :1959 test asserts wrap

#### 3. pick_best_with_anchor helper
- [ ] 加 `pub(crate) fn pick_best_with_anchor(scored: &[(usize, i64)], anchor: Option<i64>) -> Option<(usize, i64)>` 
- [ ] anchor=None → 取 scored.first().copied()
- [ ] anchor=Some(a) → scored 已按 primary+secondary 排序、直接取 first；但因 apply_fuzzy_filter 已含 secondary 排序、此 helper 退化為「取 first」+ 空時 None
- [ ] 此 helper 主要為 picker UT 直接呼 fuzzy 並驗 best 結果

#### 4. UT
- [ ] INT-fuzzy-anchor-01：chapters = mock 5 章、構造同 score 多筆命中 (idx 1 + idx 4)、anchor=Some(2) → 結果第一筆 idx=1
- [ ] INT-fuzzy-anchor-02：同上 anchor=None → 結果第一筆按 stable order 取
- [ ] INT-fuzzy-cjk-mapping-01：舊章「第 47 章 出關」、新源 toc 含「第 48 章 出關」+ 「第 100 章 出關」、anchor=Some(47) → best 是「第 48 章」
- [ ] INT-fuzzy-cascade-reader-01：跑 cargo test reader::tests::int_mode_02 / int_mode_03 / int_mode_04 全綠
- [ ] INT-fuzzy-cascade-reader-02：跑 int_filter_03_* + int_toggle_02 全綠
- [ ] cargo test reader 模組整 group 通過

#### 5. baseline 驗證
- [ ] `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo test reader::tests 2>&1 | tail -20` 全綠
- [ ] cargo build 無新 warning

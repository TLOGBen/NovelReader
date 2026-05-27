# Goal

## 目標（Goal）

在 shelf 或 reader 內按 `s` 跳出 SearchPickerScreen — 並行搜尋所有 enabled 書源、每回來一個結果即時 append 到 picker 列表；使用者 Enter 選一個書源命中後系統 fuzzy match 當前章節到新源 TOC、score 通過閾值就自動換源並保留進度位置；換源完成回到使用者原本所在 screen（reader-entry 回 reader 並跳對應章；shelf-entry 回 shelf 帶 toast）。完成後使用者讀到一半的書遇到原源掛掉 / 慢時，能在不離開 reader 的情況下跨源繼續讀、最少 3 個按鍵（s → Enter → y）。

## 驗收標準（Criteria）

- [ ] **C1：並行 streaming 搜尋** — 按 s 後 picker 表格邊跑邊填、最慢書源不阻塞 picker UI；超時書源（>5s）行顯灰色「逾時」、其他書源仍正常顯示
- [ ] **C2：Enter 提早選** — 使用者可在任何時候 Enter 已回來的命中行；按下瞬間 `JoinSet::abort_all()` 清剩餘搜尋、切到 Confirming phase
- [ ] **C3：Fuzzy 章節對應預覽** — Confirming phase 顯示「舊源第 X 章 《章名》 → 新源第 Y 章 《章名》(score N)」單行對應；score > 50 才接受 y confirm；< 50 顯示「找不到對應章節 (best score N < 50)」、Esc 回 Picking、不寫 DB
- [ ] **C4：Atomic 換源** — confirm 成功時 `library::facade::switch_source_tx` 一次寫完 sources + chapters + progress 三表；progress.chapter_index = fuzzy 命中的新源 idx（非寫死的 first_idx）；任一階段失敗整 tx rollback
- [ ] **C5：Caller-aware Transition（continuity 保留）** — picker 完成後依 entry：reader-entry → 重建 ReaderScreen 跳該章；shelf-entry → 回 shelf 帶 toast「已換源 X → Y，目標：第 N 章 《章名》」
- [ ] **C6：Reader Filter mode 不誤觸** — Reader 在 Filter mode（`/` 已按）下按 s 仍視為 query 字元、不開 picker
- [ ] **C7：cargo test 全綠** — 既有 118 tests + 新增 INT 全通過；既有 5 個 apply_fuzzy_filter caller（2 production + 3 test asserts）cascade 不退化；既有 switch_source_core fault-injection UT 不退化、新加 target_idx=Some(N) UT 通過
- [ ] **C8：手動驗收** — `cargo install --path . --force` 後手動跑：(a) shelf 按 s 進 picker 看 streaming / (b) reader 按 s 進 picker confirm 後回 reader 跳對應章 / (c) Filter mode s 是 query 字元 — 三項視覺/操作上可確認

## 範圍（Scope）

### 包含（In scope）

- 新檔 `src/presentation/handlers/tui/picker.rs`（SearchPickerScreen with `Phase::Picking | Confirming` + `entry: EntryMode`）
- 既有 `src/presentation/handlers/tui/reader.rs` apply_fuzzy_filter 簽名擴 `(query, chapters, anchor: Option<i64>) -> Vec<(usize, i64)>`
- 既有 `src/presentation/handlers/switch_source_core.rs::run` + `SwitchSourceDeps::switch_source_tx` trait method 簽名加 `target_idx: Option<i64>`
- 既有 `SwitchOutcome` 新加 `new_progress_chapter_name: String`
- 既有 `src/library/facade.rs::switch_source_tx` 簽名加 `target_idx: Option<i64>`
- 既有 `src/library/dao.rs::update_book_source_tx_inner` step 4 改 `target_idx.unwrap_or(first_idx)`
- 既有 `src/presentation/handlers/tui/shelf.rs` `s` 鍵 wire 從 SwitchSourceScreen 改為 SearchPickerScreen
- 既有 `src/presentation/handlers/tui/reader.rs` Normal mode `s` 鍵新加 wire 到 SearchPickerScreen
- 既有 `src/presentation/handlers/tui/mod.rs` 移除 `pub mod switch_source`、加 `pub mod picker`
- 刪除 `src/presentation/handlers/tui/switch_source.rs`（兩欄手 paste UX 整片淘汰）
- CLI handler `src/presentation/handlers/switch_source.rs`（CLI 入口）傳 `target_idx: None` 保持現行「換到第一章」行為
- per-source search timeout = 5s 硬編碼
- Edge cases 三條 UT 覆蓋：(a) 新源 toc=0 / (b) fuzzy 同分 tiebreak / (c) 舊章不在 chapters table

### 不包含（Out of scope）

- **手 paste URL manual mode** — 整片刪除既有 SwitchSourceScreen，不留 fallback；要換到「不在 enabled 書源」的源就先 `source import` 再換
- **跨 session「上次換源到哪」記憶** — 每次 picker 都重新搜尋當前所有 enabled 書源、不快取
- **使用者 reorder / 手挑章節對應** — fuzzy 閾值通過自動跳、低於閾值 abort；不開使用者最後 confirm 章節 idx 的途徑
- **章節對應預覽前後多章** — Confirming phase 只顯示舊→新單行對應 + score，不列前後候選章
- **backup / config 整合** — 換源是 reading flow 操作、不寫 backup snapshot / 不改 config.toml
- **CLI 子命令 `novel-looker switch-source`** — 既有 CLI 保留現行手 paste 流程、不加新 streaming UX 子命令（streaming + modal confirm 不適 CLI）
- **書名 / 作者 query 正規化層** — 不去「（精校版）」「全本」前綴；既有 `Scraper::search` 怎麼跑就怎麼跑
- **Picker 完成自動 reload reader 跳該章 — 跨 screen 一律自動跳** — 改為 caller-aware：reader-entry 跳 reader、shelf-entry 回 shelf；不做「永遠跳 reader」這種一刀切
- **`futures` crate dep** — 並行用 `tokio::task::JoinSet` 不引入新 dep
- **fuzzy 閾值 50 由使用者運行時調** — 硬編碼 placeholder、收尾用 throwaway script 對 3-5 本實書 dump CSV 由 user 看分布決定；user 決定後寫回 spec / const，不留 runtime 可調介面

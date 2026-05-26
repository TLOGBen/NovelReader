# Test Strategy

本次是 pure refactor，**測試重心是回歸 + 結構自檢**，不新增 feature 測試。

---

## E2E 測試策略

每個場景在「完整重構」結束後跑一次；中間態（每完成一個 step）只跑相關 subset（標 *）。

| # | 場景 | 起點 | 終點 | 對應 Criteria | 中間態跑 |
|---|------|------|------|--------------|----------|
| E1 | 編譯 + 單元測試 | `git checkout refactor-branch` | `cargo test` 全綠 | C1, C2, REQ-004 Scen 4.1, REQ-005 | * 每 step 後 |
| E2 | CLI grammar diff | 重構前 binary | `novel-looker help` 與每個 subcommand `--help` 輸出 | C3, REQ-003 Scen 3.1 | step 4 後 |
| E3 | SQLite schema diff | 重構前 DB 的 schema dump | `sqlite3 ~/.local/share/novel-looker/novel-looker.db .schema` 與 baseline 比對 | C4, REQ-003 Scen 3.2 | * 每 step 後 |
| E4 | 既有書源 import | `examples/gutenberg.json` + `book-sources/uukanshu.json` | 兩個 import 都成功，無 deserialize error | C6, REQ-003 Scen 3.3 | step 2 後 |
| E5 | config.toml round-trip | 既有 `~/.config/novel-looker/config.toml` | `config show` 顯示 `backup.backend / backup.keep / backup.local.path` 三 key | REQ-003 Scen 3.4 | step 1 後 |
| E6 | uukanshu add (Cloudflare bypass) | uukanshu 書源已 import | `add --source ... book/21940/` 印 `✓ 加入書架 (#N) 超維術士 / 佚名` | C7, REQ-004 Scen 4.2 | step 2 後 |
| E7 | uukanshu sync + read | E6 已建 novel + `$NID` 已 export | `sync $NID` 印「✓ 同步 N 章」（N 為當下 sync 結果，記入 `/tmp/sync-count.txt` 作 baseline） + `read $NID 0` 印第 1 章標題 + 內文 ≥ 50 行 | C7, REQ-004 Scen 4.3 | step 3 後 |
| E8 | backup → Drive | 設定 `backup.local.path` | `backup` 印「✓ 備份 N 本書 [local] → ...」+ 檔案存在於 path | REQ-004 Scen 4.4 | step 1 後 |
| E9 | TUI 啟動 + 切章 | E7 已 sync | `tui $NID` 開啟 ratatui，按 j/k 換章，章節列表非空（行數 ≈ `/tmp/sync-count.txt` 紀錄值） | REQ-004 (TUI subset) | step 4 後 |
| E10 | legado-converter skill 重跑 | yckceo 7321 URL | `convert.py` 產出與重構前 byte-identical 的 JSON | C10, REQ-006 Scen 6.2 | step 4 後 |
| E11 | git diff 檢查 .claude/skills/ | refactor 結束 | `git diff --stat .claude/skills/` 無輸出 | REQ-006 Scen 6.1 | step 4 後 |
| E7b | Cache hit 不重抓 | E7 已 read 一次 | 第二次 `read $NID 0`，`RUST_LOG=debug` 不出現 fetch_chapter_content log | design.md sequence diagram §3 alt cache hit | step 3 後 |
| E12 | Error context 品質 | 故意輸入壞 URL | `cargo run -- add --source https://bad.example.invalid http://bad.url` stderr 含 「URL: ...」 或「rule: ...」context 字串 | design.md 錯誤處理策略 | step 4 後 |
| E13 | TOC re-sync 不破壞 progress | E7 已 sync + 有 progress | 再跑一次 `sync $NID`，後接 `sqlite3 ... "SELECT chapter_index FROM progress WHERE novel_id=$NID"` 數字不變 | design.md library/service/shelf.rs invariant | step 3 後 |
| E14 | 連續操作不撞 lock | refactor 結束 | `cargo run -- sync $NID && cargo run -- backup` 不出 `database is locked` | design.md DB connection 共享策略 | step 4 後 |

---

## 整合測試策略

| 測試目標 | 涉及層 | 關鍵驗證點 |
|---------|--------|-----------|
| Handler → 三 context facade | Presentation handlers + Catalog/Library/Backup facades | Handler 只透過 facade 介面溝通；不直接 import service 或 dao |
| Facade → Service + DAO | facade.rs → service/* + dao.rs | facade 是唯一同時看到 service 與 dao 的層；service 與 dao 互相不見 |
| Catalog DAO + Library DAO 共享 connection | catalog/dao.rs + library/dao.rs + library/dao.rs::open_db() | 兩個 DAO 透過同一個 Connection 操作；不出現 SQLITE_BUSY 或重複 open |
| Shared Kernel 寫入順序 | catalog/dao.rs::replace_toc + library/dao.rs::save_chapter_content | `sync` 後 chapters 表 `idx/name/url` 由 Catalog 寫；`read` 後 `content` column 由 Library 寫；兩個寫不打架 |
| 舊 DB 相容 | library/dao.rs (open existing DB) | 重構前產生的 DB 檔被新 binary 開啟，不出現 migration 訊息（因為 schema 不變） |
| Shared Kernel 寫入欄位分工 | catalog/dao.rs replace_toc + library/dao.rs save_chapter_content | sync 後 `sqlite3 ... "SELECT idx,name,url,length(coalesce(content,'')) FROM chapters WHERE novel_id=N LIMIT 3"` 顯示 idx/name/url 非空、content length 為 0；read 後同 query 顯示 content length > 0 |
| 舊 backup JSON import 還原 | backup/facade::import_from + library/dao | 用重構前 export 出的 JSON 跑 `import`，書架與 progress 等價（章節需 re-sync）|

---

## 關鍵邊界條件

以下邊界條件必須在 e2e 或整合測試中**至少有一個場景覆蓋**：

- **服務不直接呼 SQL — REQ-002 Scen 2.1/2.2/2.3**
  驗證：`grep -rn "use rusqlite" src/*/service/ 2>/dev/null` 無輸出
- **Service 不依賴 DAO（更嚴格層級邊界）— REQ-002 Scen 2.2**
  驗證：`grep -rnE "use crate::[a-z]+::dao" src/*/service/ 2>/dev/null` 無輸出
- **Facade 不互呼跨 context — design.md「facade 不互呼」約束（backup→library 為 Conformist 例外）**
  驗證：`grep -rnE "use crate::(catalog|library|backup)::facade" src/ 2>/dev/null | grep -E "/facade\.rs:" | grep -v "src/backup/facade.rs"` 無輸出
- **DB connection 不重複開 — design.md DB connection 共享策略**
  驗證：`grep -rnE "Connection::open|LibraryDb::open" src/ | grep -vE "src/(main|library/dao)\.rs"` 無輸出（除 main.rs 與 library/dao.rs 外，其他 context 透過 handle 接收）
- **main.rs 不含 domain symbol — design.md Wiring 圖**
  驗證：`grep -rnE "rusqlite|wreq|BookSource|Scraper" src/main.rs` 無輸出（main.rs 純 wiring，不直接見 domain）
- **Cloudflare bypass 路徑保留 — REQ-004 Scen 4.2**
  驗證：`grep -rn "Emulation::Chrome131" src/catalog/service/scraper.rs` 恰有 1 行
- **Cache miss 流程不破壞既有行為 — REQ-004 Scen 4.3 + E7b**
  驗證：對未抓過的章節執行 `read`，能 fetch + cache + 印出；第二次執行同章節**不發** HTTP 請求（用 `RUST_LOG=debug` 確認 `fetch_chapter_content` log 不出現）
- **TRANSITION / `_legacy` 殘留清零 — design.md Verification 段**
  驗證：`grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/ 2>/dev/null` 無輸出
- **舊資料 schema 相容 — REQ-003 Scen 3.2**
  驗證：用重構前的 `.db` 檔執行 `shelf / read` 等不出現 column not found
- **編譯零 new warning — REQ-005 Scen 5.1**
  驗證：基線 `cargo build` warning 數量 = 2（dead_code: select_within, extract_all_doc）；重構後 ≤ 2
- **Mod 路徑變動不留 dead use — REQ-005 Scen 5.1**
  驗證：`cargo build` 不出現 unused_imports
- **Cloudflare bypass 路徑保持 — REQ-004 Scen 4.2**
  驗證：Catalog service::scraper 的 `wreq::Client::builder().emulation(Emulation::Chrome131)` 程式碼搬移後仍存在；對 uukanshu HTTP 請求收到 200 OK（非 403）
- **Plugin 不需修改 — REQ-006 Scen 6.1**
  驗證：`git diff --stat .claude/skills/` 在整個 refactor PR 中為空
- **Reading session state 暫居 Presentation 不洩 — design.md ReaderApp 註解**
  驗證：`grep -rn "scroll_offset\|chapter_index" src/library/ 2>/dev/null` 只應出現在 `ReadProgress` struct 定義與 DAO 序列化處；不在 Library service 業務邏輯內出現（除非透過 `ReadProgress` 整包傳遞）

---

## 驗證執行順序

每完成一個 refactor step：

1. `cargo check`（必過）
2. `cargo build --bin novel-looker`（必過、warning 不增加）
3. `cargo test`（必綠）
4. 跑該 step 對應的 e2e 子集（見上表「中間態跑」欄）
5. 跑該 step 對應的整合測試自檢（grep 類）

全部完成後：

6. 跑完整 E1-E11
7. 跑所有整合測試項
8. 用 `git diff --stat` 確認 `.claude/skills/` + `.claude/wip/` + `.claude/analyze/` 三目錄無改動

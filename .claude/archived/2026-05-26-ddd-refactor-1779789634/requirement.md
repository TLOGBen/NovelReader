# Requirements

從 `goal.md` 的 10 條 Criteria 拆出 6 條離散需求。每條需求對應一組可觀察情境。

---

## REQ-001: 目錄結構符合 DDD 藍圖

**描述**：重構後 `src/` 下出現 4 個 bounded context 目錄（catalog/ library/ backup/ presentation/）+ utils/，每個 context 內按需要含 dao.rs、service/、facade.rs；既有檔案按 `.claude/wip/ddd-analysis.md` §4 File-to-Context Mapping 全數搬完，根目錄只剩 main.rs / config.rs / utils 入口。

### Scenarios

**Scenario 1.1: 目錄結構檢查**
- **Given** 重構完成的 codebase
- **When** 執行 `find src -type d -mindepth 1 -maxdepth 1 | sort`
- **Then** 輸出包含 `src/catalog`, `src/library`, `src/backup`, `src/presentation`, `src/utils` 五個目錄
- **And** 不包含 `src/source`（已搬入 catalog）

**Scenario 1.2: Context mod.rs 存在且註解職責**
- **Given** 4 個 context 目錄都建立
- **When** Read `src/catalog/mod.rs` / `src/library/mod.rs` / `src/backup/mod.rs` / `src/presentation/mod.rs`
- **Then** 每個檔案最上方有 `//!` doc comment 描述該 context 的 Purpose 與對外 PL（從 wip/ddd-analysis.md §3 Canvas 萃取）

**Scenario 1.3: 舊扁平檔案已搬移**
- **Given** 重構完成
- **When** 列出 `src/` 第一層 `.rs` 檔
- **Then** 只剩 `main.rs` 與 `config.rs`（utils 是目錄）；不再有 `cli.rs`, `scraper.rs`, `storage.rs`, `backup.rs`, `reader.rs`, `models.rs`

---

## REQ-002: Service 層與 DAO 層的依賴隔離

**描述**：每個 context 內 service 層的 .rs 檔不直接 import SQL 相關 crate（`rusqlite`）也不 import 任何 dao module；DAO 是唯一接觸 SQL 的層；facade 是唯一同時呼叫 service 與 DAO 的層。

### Scenarios

**Scenario 2.1: service 不 import rusqlite**
- **Given** 重構完成
- **When** 執行 `grep -rn "use rusqlite" src/*/service/ 2>/dev/null`
- **Then** 無輸出

**Scenario 2.2: service 不 import 任何 dao module**
- **Given** 重構完成
- **When** 執行 `grep -rnE "use crate::[a-z]+::dao" src/*/service/ 2>/dev/null`
- **Then** 無輸出

**Scenario 2.3: DAO 是唯一 import rusqlite 的層**
- **Given** 重構完成
- **When** 執行 `grep -rln "use rusqlite" src/ | grep -vE "(dao|storage)\.rs"`
- **Then** 無輸出（rusqlite 只在 dao.rs / 共用 storage helper 出現）

**Scenario 2.4: facade 可同時呼 service + DAO**
- **Given** 重構完成
- **When** Read 任一 facade.rs（例如 `src/catalog/facade.rs`）
- **Then** 該檔案 import 自己 context 的 service 與 dao 模組（允許），或 import 其他 context 的 facade（允許），但不直接 import 其他 context 的 service / dao

---

## REQ-003: 對外介面 100% 不變

**描述**：CLI grammar（subcommand / flag / help text）、SQLite schema、book-source JSON 欄位、config.toml key 五項使用者可見契約逐字不變。

### Scenarios

**Scenario 3.1: CLI subcommand 列表不變**
- **Given** 重構完成
- **When** 執行 `cargo run -- --help`
- **Then** 輸出列出 `source / search / add / shelf / sync / read / tui / config / export / import / backup / help` 12 個 subcommand（與重構前一致）

**Scenario 3.2: SQLite schema 不變**
- **Given** 對重構前的 binary 跑過 `source import / add / sync` 產生資料，存在 `~/.local/share/novel-looker/novel-looker.db`
- **When** 重構後 binary 開啟同 DB 並執行任一查詢
- **Then** 不出現「no such column」或 schema mismatch 錯誤
- **And** `sqlite3 .schema` 輸出與重構前完全一致

**Scenario 3.3: 既有書源 JSON 可直接 import**
- **Given** `book-sources/uukanshu.json` 與 `examples/gutenberg.json`（重構前產出）
- **When** 重構後 binary 執行 `source import examples/gutenberg.json` 與 `source import book-sources/uukanshu.json`
- **Then** 兩個 import 都成功，無 deserialize 錯誤

**Scenario 3.4: config.toml 既有 key 可讀**
- **Given** `~/.config/novel-looker/config.toml`（含 backup.backend、backup.keep、backup.local.path）
- **When** 重構後 binary 執行 `config show`
- **Then** 輸出含上述三 key 與原值

---

## REQ-004: 既有功能行為等價

**描述**：所有 CLI subcommand 在重構後的行為（成功路徑與錯誤訊息）與重構前一致；e2e 端到端流程跑通。

### Scenarios

**Scenario 4.1: 已知 cargo test 全綠**
- **Given** 重構完成
- **When** 執行 `cargo test`
- **Then** 退出碼為 0；`source::rule::tests` 4 個案例通過（parse_basic / parse_attr_and_replace / parse_alternatives / extract_text_with_fallback）

**Scenario 4.2: Cloudflare bypass 仍生效**
- **Given** 重構完成 + 已 import uukanshu 書源
- **When** 執行 `cargo run -- add --source https://uukanshu.cc https://uukanshu.cc/book/21940/`
- **Then** 輸出 `✓ 加入書架 (#N) 超維術士 / 佚名`（書名 / 作者非 Unknown）

**Scenario 4.3: TOC 同步 + 章節讀取等價**
- **Given** Scenario 4.2 已加入超維術士到書架
- **When** 執行 `cargo run -- sync N` 接著 `cargo run -- read N 2838`
- **Then** sync 印出「✓ 同步 N 章」（N 為當下站點實際章節數，記入 setup.md baseline）；read 印出對應章節 title + 內文文字 ≥ 50 行

**Scenario 4.4: 備份流程等價**
- **Given** 重構完成 + 設定 `backup.local.path = /mnt/g/我的雲端硬碟/novel-looker-backup`
- **When** 執行 `cargo run -- backup`
- **Then** 輸出「✓ 備份 N 本書 [local] → ...novel-looker-YYYYMMDDTHHMMSSZ.json」；該檔案實際存在於 backup.local.path

**Scenario 4.5: 舊 backup JSON 可被新 binary 還原（goal C5 對應）**
- **Given** 重構前的 binary 跑過 `export /tmp/pre-refactor.json` 留下 2 本書（含 progress）
- **When** 重構完成後新 binary 執行 `cargo run -- import /tmp/pre-refactor.json`
- **Then** 輸出「✓ 匯入 2 本書（N 含進度） ← /tmp/pre-refactor.json」
- **And** `cargo run -- shelf` 顯示這 2 本書（書名 / 作者 / source_url 與重構前一致）
- **And** 跑 `cargo run -- sync` 補章節後，`cargo run -- read N idx` 對重構前留下的 progress.chapter_index 仍能正常讀出內文

---

## REQ-005: 編譯品質不退化

**描述**：重構後編譯不出現新 warning；既有 warning（如 `select_within` dead_code）數量不增加。

### Scenarios

**Scenario 5.1: 編譯 warning 不增加**
- **Given** 重構前 `cargo build --bin novel-looker` 產生 N 條 warning（基線：2 條 dead_code）
- **When** 重構後 `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker`
- **Then** warning 數量 ≤ N
- **And** 無 unused_imports、no_use 之類 module 搬移留下的痕跡

**Scenario 5.2: 無 cargo check error**
- **Given** 重構任一階段完成（每個 step 後）
- **When** 執行 `cargo check`
- **Then** 退出碼為 0

---

## REQ-006: Plugin layer（.claude/skills/）不受影響

**描述**：parse-novel-site / legado-converter / add-to-shelf 三個 SKILL.md 與 scripts/ 內容**不修改**仍能透過新 binary 的 CLI 運作。

### Scenarios

**Scenario 6.1: skill 檔案無變動**
- **Given** 重構過程中
- **When** 執行 `git diff --stat .claude/skills/`
- **Then** 無輸出（無檔案改動）

**Scenario 6.2: legado-converter 仍可端到端跑**
- **Given** 重構完成
- **When** 執行 `python3 .claude/skills/legado-converter/scripts/convert.py "https://www.yckceo.com/yuedu/shuyuan/content/id/7321.html" --out-dir /tmp/skill-test/`
- **Then** 產出 `/tmp/skill-test/笔趣_.json` 且能用新 binary `source import` 成功

**Scenario 6.3: add-to-shelf skill 流程仍走通**
- **Given** 重構完成 + uukanshu 書源已 import
- **When** 依 `.claude/skills/add-to-shelf/SKILL.md` 步驟跑 `source list → add → sync → read`
- **Then** 各步驟輸出符合 SKILL.md 說的格式（如「✓ 加入書架 (#N) 書名 / 作者」）

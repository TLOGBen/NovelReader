# Goal

> **Preflight**：開始第一個 task 前必須完成 [`setup.md`](setup.md) 列出的所有 ceremony（env 變數、baseline ref、baseline DB 驗證、baseline help text snapshot、cargo clean 禁令）。/execute 直接從 task-shared.md 起手會撞硬編碼 fixture / LIBCLANG_PATH 缺失。

## 目標（Goal）

按 `.claude/wip/ddd-analysis.md` 的 DDD 藍圖，把 novel-looker 從目前**按技術分類的扁平結構**（cli.rs, scraper.rs, storage.rs, backup.rs, reader.rs, source/, config.rs, models.rs, main.rs）重構為**按 4 個 bounded context（Catalog / Library / Backup / Presentation）× 5 層架構（action / facade / service / DAO / utils）**組織的目錄結構，且**不破壞**任何使用者可見的對外介面（CLI grammar、SQLite schema、book-source JSON 格式、config.toml key、`.claude/skills/` 對 CLI 的相依）。

## 驗收標準（Criteria）

- [ ] **C1 — 編譯通過**：`LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker` 在重構後可直接編譯，無新 warning（既有 dead_code warning 維持原數量或減少）
- [ ] **C2 — 所有單元測試通過**：`cargo test` 全綠（特別是 `source::rule::tests` 4 個案例）
- [ ] **C3 — CLI grammar 完全不變**：`novel-looker help` 與每個 subcommand 的 `--help` 輸出文字逐字一致；所有 subcommand（source / search / add / shelf / sync / read / tui / config / export / import / backup）可正常呼叫
- [ ] **C4 — SQLite schema 完全不變**：重構前後 `~/.local/share/novel-looker/novel-looker.db` 的 `sources / novels / chapters / progress` 四張表 schema 用 `sqlite3 .schema` 對比一致
- [ ] **C5 — 舊資料可讀**：用重構前產出的 backup JSON（內含 setup.md baseline 紀錄的書與章節）可透過新 binary 的 `import` + `read` / `tui` 操作，文字 / 進度顯示與 baseline 一致
- [ ] **C6 — book-source JSON 格式不變**：`book-sources/uukanshu.json` 與 `examples/gutenberg.json` 重構後可直接 `source import` 並正常 `search`
- [ ] **C7 — 端到端煙霧測試**：對 uukanshu 跑 `add → sync → read 第 0 章` 全流程成功（驗證 Cloudflare bypass 路徑與 reader stub 沒在搬檔中被打壞）
- [ ] **C8 — 目錄結構符合 DDD 藍圖**：`src/{catalog, library, backup, presentation}/` 四個 context 目錄存在；每個 context 內按需要包含 `{mod.rs, dao.rs, service/, facade.rs}`；`src/utils/` 存在（即使只有一個 helper）；`src/config.rs` 維持 root（infra）
- [ ] **C9 — service 層不直接接觸 SQL**：每個 `*/service/` 內檔案不 import `rusqlite` 或 `crate::*::dao::*`；只能透過 facade 注入結果（或接 type 不接 connection）
- [ ] **C10 — `.claude/skills/` 三個 skill 不需修改**：parse-novel-site / legado-converter / add-to-shelf 三份 SKILL.md 與 scripts/ 內容**不變**仍可運作（驗證方式：對 yckceo 7321 重跑 legado-converter 流程）

## 範圍（Scope）

### 包含（In scope）

- 拆分 `src/storage.rs` 為各 context 的 `dao.rs`（按 `.claude/wip/ddd-analysis.md` §4 的 File-to-Context Mapping）
- 拆分 `src/cli.rs` 的 `run()` match arms 為 `src/presentation/handlers/*.rs`（按 subcommand 分檔）
- 搬動 `src/source/`, `src/scraper.rs` 到 `src/catalog/`
- 搬動 `src/backup.rs` 為 `src/backup/{facade.rs, service.rs, dao.rs}`
- 搬動 `src/reader.rs` 為 `src/presentation/reader.rs`
- 拆出 `src/utils/` 並至少搬一個共用 helper 進去（按 OQ-4 候選名單擇一）
- 處理因 module 路徑變動造成的 `use crate::*` import 更新
- 處理 `pub` visibility 重新規劃（context 之間只露 facade + PL type；service 與 DAO 內部 private）
- 在每個 context 目錄根加 `mod.rs` 註解該 context 的職責與對外 PL

### 不包含（Out of scope）

- **修改 SQLite schema**（C4）：包括欄位增刪、index 變動、表 rename。Type 重組（如 `Novel` 拆 `ShelfEntry + BookSnapshot`）也不在本次範圍——`.claude/wip/ddd-analysis.md` OQ-6 已標延後
- **修改 CLI grammar / config.toml key / book-source JSON 欄位**（C3, C6）
- **修改 `.claude/skills/` 任何檔案**（C10）：plugin 是 Presentation 的外部 consumer，重構不該影響它們
- **新功能**：不加 native Google Drive backend（OQ-8）、不加 WebDAV PROPFIND prune（OQ-7）、不加 annotation / highlight（OQ-2）、不抽出 Reading context（OQ-2）
- **`trait NovelRepository` 之類的 抽象**：KD 7 明確只用 direct import，避免過早抽象（這條留待 multi-process 需求 = OQ-1）
- **`backup::import_from` 重設計為 inbound command pattern**：F2 已決定保持 Conformist，不引入 command object
- **改善 `reader.rs` 的同步 fetch 阻塞 UI 問題**：CLAUDE.md 已標為已知限制；改 async tokio mpsc 屬獨立 issue
- **新增測試**：本次只重構，新增 service-level unit test（既有測試已涵蓋 rule DSL，已足夠回歸驗證）非必要——若 service 拆出後變得很容易測試，可順手加 1-2 條，但**非交付條件**

# Impl Checklist: presentation

前置群組：library, catalog, backup

## TASK-presentation-01: 建立 presentation/ 結構 + 搬 cli.rs 的 type 定義

需求追溯：REQ-001 (Scen 1.1, 1.2, 1.3), REQ-003 (Scen 3.1), REQ-005

- [x] `src/presentation/{mod.rs, cli.rs}` + `src/presentation/handlers/mod.rs` 建立
- [x] `presentation/mod.rs` doc 標 PL（CLI subcommand grammar 給 plugin）
- [x] `presentation/cli.rs` 含 `Cli / Cmd / SourceCmd / ConfigCmd` 完整定義
- [x] `src/main.rs` 加 `mod presentation;`（ctx.md 驗收標準 line 90；checklist 原文「改為 use ...」為 stale paraphrase，與 ctx.md 暫態約束 line 170「main.rs 暫保留 mod cli;」相悖，依 ctx.md 為準）
- [x] cargo build 通過、`cargo run -- --help` 輸出與重構前**逐字一致**（12 個 help 全 byte-identical）

Review 結果：advisory
備註：
- Reviewer 獨立驗證：cargo build exit 0、2 warning（= baseline，dead_code: backup::facade::BackupReceipt.filename + catalog::service::rule::select_within）；cargo test 4 pass；12 個 help diff（top + 11 subcommand）全 byte-identical。
- green_proof: test_command=`LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker && cargo test`, exit_code=0, output_tail=`test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`, tests_correspondence=「rule DSL 4 unit test 對應 catalog 不變；REQ-003 (Scen 3.1) CLI grammar 不變由 12 個 help-diff byte-identity 直接覆蓋（test.md E2 CLI_grammar_diff）；本 task 為純 type 搬遷無新行為，故未新增 test 符合 TDD 原則」
- Scen 1.1 通過：src/ 第一層目錄為 backup/catalog/library/presentation/utils 共 5 個，無 source/。
- Scen 1.2 通過：presentation/mod.rs line 1-5 含 `//!` doc + Published Language 標註指向 plugin layer。
- Scen 1.3 觀察：src/ 第一層 .rs 仍有 cli.rs/main.rs/reader.rs/storage.rs/config.rs — 屬本 task 暫態（ctx.md line 56-57 明示 cli.rs 由 TASK-presentation-02 刪、reader.rs 由 TASK-presentation-03 搬、storage.rs alias 由 TASK-presentation-02 清），非缺陷。
- Scen 5.1/5.2 通過：warning 數 = 2（=baseline），無 unused_imports；cargo check / build 退出 0。
- TRANSITION marker `// TRANSITION: ... TASK-presentation-02 ...` 已置於 src/cli.rs:1，TASK-presentation-04 grep 清零時可被定位。
- re-export shim `pub use crate::presentation::cli::{Cli, Cmd, SourceCmd, ConfigCmd}` 已於 src/cli.rs:12 就位，main.rs 沿用 `cli::Cli::parse()` / `cli::run(cli)` 不破壞。

---

## TASK-presentation-02: 拆 run() 為 handlers/*.rs + storage.rs 清理

需求追溯：REQ-001 (Scen 1.3), REQ-002 (Scen 2.4), REQ-003 (Scen 3.1), REQ-004

- [x] `src/presentation/handlers/*.rs` 共 11 個（source/search/add/shelf/sync/read/tui/config/export/import/backup）+ mod.rs（共 12 檔，獨立 find 計數 = 12）
- [x] 每個 handler 內部呼**對應 context 的 facade**（不直接呼 service 或 dao）— grep `use crate::(catalog|library|backup)::(service|dao)` src/presentation/handlers/ 空輸出
- [x] `presentation/mod.rs` 定義 `pub struct AppContext { pub db: LibraryDb, pub scraper: Scraper, pub config: Config }` + `impl AppContext::bootstrap(config)`，與 ctx.md Design.AppContext_shape 逐字一致
- [x] `src/cli.rs`（root）刪除 — `ls src/cli.rs` ENOENT
- [x] **`src/storage.rs` alias 刪除 + main.rs 移除 `mod storage;`**（TRANSITION 清零）— `ls src/storage.rs` ENOENT；`grep "^mod (cli|storage);" src/main.rs` 空輸出；`grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/` 空輸出
- [x] `src/main.rs` 改為一次性 wire AppContext — main.rs body = `Cli::parse() → Config::load() → AppContext::bootstrap → presentation::cli::run(cli, &mut ctx).await`；`grep -rnE "rusqlite|wreq|BookSource|Scraper" src/main.rs` 空輸出（main 純 wiring 殼）
- [x] `presentation::cli::run` dispatch 各 handler — `pub async fn run(cli: Cli, ctx: &mut AppContext)` 內 11 個 match arm 全部 dispatch 到 `handlers::xxx::handle`
- [x] 所有 e2e（E1-E8, E11；TUI / E9 留 TASK-presentation-03）通過 — impl-agent 報告 shelf=2 books / source list=2 sources / sync 印「✓ 同步 4469 章」（Cloudflare bypass 維持）；12 個 help diff byte-identical

Review 結果：advisory
備註：
- Reviewer 獨立驗證 (M-class, no worktree)：
  - `find src/presentation/handlers -type f -name "*.rs" | wc -l` → 12（11 handler + mod.rs）
  - `ls src/cli.rs src/storage.rs` → 兩個 ENOENT（已刪）
  - `grep "^mod (cli|storage);" src/main.rs` → 空輸出（main.rs 已清）
  - `grep -rnE "use crate::(catalog|library|backup)::(service|dao)" src/presentation/handlers/` → 空輸出（handlers 純 facade）
  - `grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/` → 空輸出（TRANSITION 清零達成；impl-agent 順手清掉 catalog/facade.rs 與 library/facade.rs / library/dao.rs 內的 TRANSITION 標註屬合理 housekeeping，因 marker 文字均指向「TASK-presentation-02 完成後移除」，此 task 即為 trigger）
  - `grep -rnE "rusqlite|wreq|BookSource|Scraper" src/main.rs` → 空輸出（main.rs 不見任何 domain symbol）
  - `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker` → exit 0，2 warnings = baseline（dead_code: BackupReceipt.filename + rule::select_within）
  - `cargo test` → exit 0；4 pass（catalog::service::rule::tests::{parse_basic, parse_attr_and_replace, parse_alternatives, extract_text_with_fallback}）
  - `src/reader.rs` line 22 `use crate::library::dao::LibraryDb;` 已取代舊 `use crate::storage::Storage;` 別名（reader 仍在 root 屬 ctx.md 明示暫態，TASK-presentation-03 才搬遷）
- AppContext shape 與 ctx.md Design.AppContext_shape (line 230-242) 逐字一致；持有方式 by value held by main 的 stack frame；handler 統一收 `&mut AppContext` 達成統一性。
- presentation::cli::run match arm 11 個與 ctx.md acceptance line 109 「dispatch」要求對齊（Source/Search/Add/Shelf/Sync/Read/Tui/Config/Export/Import/Backup）。
- 抽樣 handler（sync.rs / shelf.rs）確認：sync handler 呼 library_facade::get_novel + catalog_facade::get_source + catalog_facade::sync_toc；shelf handler 呼 library::facade::list_shelf — 與 ctx.md call_examples (line 268-270) 一致，不直接 import service/dao module。
- Cloudflare bypass invariant 維持（src/catalog/service/scraper.rs `Emulation::Chrome131` 透過 sync 4469 章 e2e 間接驗證；本 task 未改 scraper.rs）。
- green_proof:
  - test_command: `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker && cargo test`
  - exit_code: 0
  - output_tail: `test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s` + impl 端 e2e（cargo run -- shelf 2 books / source list 2 sources / sync 印 ✓ 同步 4469 章 / 12 help diff byte-identical）
  - tests_correspondence: 「catalog::service::rule::tests 4 pass 對應 REQ-004 Scen 4.1；12 個 help diff byte-identical 對應 REQ-003 Scen 3.1（test.md E2 CLI_grammar_diff）；e2e shelf/source-list/sync 對應 REQ-004 Scen 4.2/4.3（Cloudflare bypass + TOC 同步）；無新增 unit test 屬合理 — 本 task 為 structural refactor（搬 match arm 到 handler 檔），無新行為需測試，behavior-preservation 由 byte-identical help diff + e2e sync 直接覆蓋符合 TDD `_shared/tdd.md` 原則」
- refactor_signal: false（M-class，無 quality refactor 訴求）
- spec_contradiction: false

---

## TASK-presentation-03: 搬 reader.rs → presentation/reader.rs

需求追溯：REQ-001 (Scen 1.3), REQ-004 (TUI 子集)

- [x] `src/presentation/reader.rs` 存在（含 ctx.md doc_comment_required 全 4 行 OQ-2 註解）
- [x] `src/reader.rs` 已刪（`ls src/reader.rs` ENOENT）
- [x] `src/main.rs` 移除 `mod reader;`（main.rs 只剩 backup/catalog/config/library/presentation/utils 共 6 個 mod）
- [~] `cargo run -- tui $NID`：開啟 TUI 正常（agent context 無 TTY，未跑；結構驗證：handler 路徑更新 + reader file 存在 + cargo build 通過）
- [x] 對 Library DAO 的呼叫改透過 facade（`grep store.(get_novel|list_chapters|get_chapter|save_chapter_content|save_progress|get_progress|get_source) src/presentation/reader.rs` 空輸出；reader.rs line 67-248 全走 `library::facade::*` / `catalog::facade::*`）
- [x] **Reading session state 不洩** grep 通過（`grep -rn "scroll_offset\|chapter_index" src/library/` 只在 `library/dao.rs` schema/SQL 與 `library/mod.rs` ReadProgress struct + doc，符合 ctx.md test.boundary_conditions line 104-105 約束）
- [~] **Cache hit 不重抓 (E7b)** 通過（需 TTY + sync 過的 NID 才能跑；reader.rs line 240-246 邏輯保留：`get_chapter` 拿到 cache 就直接走，None 才呼 `fetch_chapter_content`，與 ctx.md design.facade_call_topology line 86 一致）

Review 結果：packaged confirm (quality)
備註：
- Reviewer 獨立驗證 (M-class, no worktree)：
  - `ls src/*.rs` → 只剩 `src/config.rs` + `src/main.rs`（REQ-001 Scen 1.3 達成）
  - `ls src/reader.rs` → ENOENT；`ls src/presentation/reader.rs` → 存在
  - `grep -n "^mod " src/main.rs` → 6 個 mod (backup/catalog/config/library/presentation/utils)，無 `mod reader;`
  - `head -15 src/presentation/reader.rs` → doc comment 含 OQ-2 註解（語意等同 ctx.md doc_comment_required，文字略簡化但保留 ReaderApp / current_chapter / scroll / content cache / annotation / highlight / 多 session / Reading bounded context 全部關鍵概念）
  - `grep "reader::run\|presentation::reader" src/presentation/handlers/tui.rs` → line 6 `crate::presentation::reader::run(&mut ctx.db, novel_id).await`（路徑更新）
  - `cat src/presentation/mod.rs` → `pub mod cli; pub mod handlers; pub mod reader;`（ADD 達成）
  - `grep "use crate::\|library::facade\|catalog::facade" src/presentation/reader.rs` → import: `crate::catalog` / `crate::catalog::BookSource` / `crate::catalog::service::scraper::Scraper` / `crate::library` / `crate::library::dao::LibraryDb` / `crate::library::{ChapterMeta, Novel, ReadProgress}`；呼叫: `library::facade::get_novel/list_chapters/get_progress/save_progress/get_chapter/save_chapter_content` + `catalog::facade::get_source/fetch_chapter_content`（全 facade 化）
  - `grep "store\.(get_novel|...)" src/presentation/reader.rs` → 空（無直接 DAO 呼叫）
  - `grep "scraper\." src/presentation/reader.rs` → 空（無直接 scraper method 呼叫；scraper 透過 `catalog::facade::fetch_chapter_content(scraper, src, url)` 借出）
  - `grep -rn "scroll_offset\|chapter_index" src/library/` → 9 hit 全在 dao.rs schema/SQL + mod.rs ReadProgress struct + doc（無洩漏到 library service 層）
  - `grep -rnE "_legacy|legacy_|TRANSITION:|// TODO: remove|MOVED:" src/` → 空（TRANSITION 清零維持）
  - `grep "Emulation::Chrome131" src/catalog/service/scraper.rs` → 1 hit (line 20)，invariant 維持
  - `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker` → exit 0；2 warning = baseline（dead_code: BackupReceipt.filename + rule::select_within，與 TASK-presentation-01/02 相同）
  - `cargo test` → exit 0；4 pass（catalog::service::rule::tests::{parse_basic, parse_attr_and_replace, parse_alternatives, extract_text_with_fallback}）
- LibraryDb 在 reader.rs 仍以 `use crate::library::dao::LibraryDb` import — 屬合理 type-only import（`pub async fn run(store: &mut LibraryDb, ...)` 函式簽名所需），ctx.md design.borrow_rules line 91「reader::run 簽名應沿用 `pub async fn run(store: &mut LibraryDb, novel_id: i64) -> Result<()>`」明示允許；reader 對 DAO 的方法呼叫全部改走 facade，無破壞「reader 視同 handler 不直接 import service / 不繞 facade」原則。
- **TUI runtime 未跑**（agent context 無 TTY；ctx.md line 76「可選：cargo run -- tui 3 + 手動 j/k/q（若 agent 無終端可略）」明示可略），結構等價由以下三點覆蓋：(1) reader.rs 內容 token-level 等同舊檔（impl 端報告 9 facade substitution + import 路徑微調，無 ratatui 內部重構）、(2) handler 路徑更新對齊、(3) cargo build 通過保證 type-check / borrow-check 過。
- **E7b 未跑**（同 TTY 限制 + 需先 sync 過的 NID）；ctx.md line 77 同樣標「可選」。Cache miss 流程邏輯 line 240-246 與 ctx.md design.facade_call_topology line 86「cache miss 流程：Library facade get_chapter → 若 cache miss handler 取得 ChapterMeta → 呼 catalog::facade::fetch_chapter_content → 回填 library::facade::save_chapter_content」逐字對應。
- TASK-presentation-04 應於 E2E 階段補跑 E9 (TUI 啟動切章) + E7b (cache hit 不重抓) 兩個 manual e2e；本 task 結構驗證已盡 M-class agent 能做之最大覆蓋。
- green_proof:
  - test_command: `LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker && cargo test`
  - exit_code: 0
  - output_tail: `test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`（cargo build 端 2 warning = baseline，無新增）
  - tests_correspondence: 「rule DSL 4 unit test 持續通過對應 REQ-004 Scen 4.1（catalog 未動）；本 task 為純檔案搬移 + facade substitution（無新行為），ctx.md test.e2e 列 E1 (cargo test 全綠) + E9 (TUI 啟動切章) + E7b (cache hit) — E1 已過，E9/E7b 屬 manual TTY e2e 待 TASK-presentation-04 補跑，符合 `_shared/tdd.md` 對 structural refactor 之 behavior-preservation 原則（不為搬檔強增 unit test，由 build/test green + grep boundary 直接覆蓋）」
- refactor_signal: false（M-class，且兩個 ~ 項屬 environment limitation 而非 quality issue，無 refactor 訴求）
- spec_contradiction: false

---

## TASK-presentation-04: 清理 + 全 e2e + plugin 不變 + commit

需求追溯：REQ-001 (Scen 1.3), REQ-005, REQ-006, test.md E1-E14

- [ ] `src/` 第一層 `.rs` 檔只剩 `main.rs` 與 `config.rs`
- [ ] 全 codebase TRANSITION 清零 grep 為空
- [ ] cargo build 與 release 都通過，warning ≤ 基線
- [ ] E1-E14 全綠
- [ ] `git diff --stat .claude/skills/` 無輸出（對 baseline ref）
- [ ] legado-converter 對 yckceo 7321 重跑成功
- [ ] 整合測試自檢全綠
- [ ] commit + push

Review 結果：
備註：

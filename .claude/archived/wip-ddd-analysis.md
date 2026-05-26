# novel-looker DDD 分析

**用途**：作為日後 `/analyze` → `/execute` 重構的策略藍圖。**不動 SQLite schema、CLI grammar、JSON 格式、config.toml key**；允許重組型別與 module。

**狀態**：已通過 `/think` Stage G + `/review`（architecture + quality + adversarial）批准。

---

## 1. Context Map

```
                ┌────────────────────────────────────────────┐
                │      Presentation (CLI + TUI)              │
                │   Gateway — Generic subdomain              │
                └─┬──────────┬─────────────────┬────────────┘
                  │          │                 │
        facade 呼叫│   facade 呼叫│      facade 呼叫│
                  ▼          ▼                 ▼
        ┌─────────────┐  ┌──────────┐  ┌──────────────┐
        │   Catalog   │  │ Library  │  │   Backup     │
        │  Supporting │  │   Core   │  │   Generic    │
        └─────┬───────┘  └────┬─────┘  └──────┬───────┘
              │               │               │
              │  PL: SearchHit,             Conformist
              │      NovelInfo,               + ACL
              │      Vec<ChapterMeta>,      (順從 Storage
              │      RawContent              既有 mutation
              │                              API；
              │  ACL @ Library 端               BackedUpNovel
              │  (raw → ShelfEntry/Chapter)    ↔ Novel mapping
              ▼                                即是 ACL)
       Shared Kernel ◀──────────────────────────┘
       (sources 表、chapters 表的 column；
        兩 context 都直接讀寫同一 SQL row)
              ▲
              │
        Library 借表給 Catalog 存
        BookSource registry
```

**關係詞註解**

| 關係 | 來源 → 目標 | 為何選這個詞 |
|---|---|---|
| **Shared Kernel** | Catalog ⇄ Library | `sources` 表 column 是 Catalog 概念（`url`/`name`/`enabled`/`json`）；`chapters` 表 column 是 Catalog（TOC：`idx`/`name`/`url`）+ Library（cache：`content`）混合。兩 context 直接讀寫同 row。不是純 C/S 因為 Library 不只是「supplier of persistence」，它已經把 Catalog 概念 import 進自己的 schema |
| **PL (Published Language)** | Catalog → Library | Catalog 對外承諾的型別：`SearchHit`, `NovelInfo`（目前以 `Novel` 雙用，未來可拆 `CatalogNovelDraft`）, `Vec<ChapterMeta>`, `RawContent`（目前是 `String`） |
| **ACL (Anti-Corruption Layer)** | Library 端 | Library 接收 Catalog PL 後翻成自家 entity；目前隱式存在於 `scraper.rs` / `cli.rs` 的 use case 流程內 |
| **Conformist + ACL** | Backup → Library | Backup 順從 `Storage::upsert_novel / save_progress` 既有 mutation API（不引入 inbound command pattern）；`backup::import_from` 內 `BackedUpNovel ↔ Novel` 的 mapping 即是 ACL |
| **(隱式) Plugin layer outbound** | Presentation → 外部 Claude skills | CLI subcommand grammar 是 Presentation 對 plugin 的 PL；plugin 不在 Rust binary 內，不算 internal context |

---

## 2. 五層架構

**橫向技術分層**，與「縱向 bounded context 切片」正交。每個 context 內都有自己的五層切片。

| 層 | 職責 | 不能做 |
|---|---|---|
| **action** | parse CLI args / 接 TUI keypress / 格式化輸出 | 業務邏輯 |
| **facade** | 編排一個 use case（呼 service + DAO） | 直接寫 SQL |
| **service** | 純 domain logic（規則 DSL、scraping pipeline、scroll 計算） | **直接碰 SQL** |
| **DAO** | SQL 存取（CRUD、transaction） | 業務 invariants |
| **utils** | 跨 context 共用 helper（URL resolve / HTML normalize / filename sanitize） | 依賴任何 domain type |

**Facade 呼 DAO + Service 是設計重點**：service 不知道 SQL 存在，testable in isolation。

---

## 3. Bounded Context Canvases

### 3.1 Catalog

| 欄位 | 內容 |
|---|---|
| **Purpose** | 描述「如何從某個小說網站抽資料」並執行抽取 |
| **Strategic Classification** | Supporting |
| **Domain Roles** | Specification（BookSource 規則定義）+ Execution（Scraper 跑規則） |
| **Inbound** | `ImportBookSource(json)`, `ListBookSources()`, `GetBookSource(url)`, `EnableBookSource(url)`, `Search(keyword, source?)`, `FetchNovelInfo(book_url)`, `FetchToc(toc_url)`, `FetchChapterContent(chapter_url)` |
| **Outbound** | PL emits: `SearchHit`, `NovelInfo`（目前 `Novel` 雙用）, `Vec<ChapterMeta>`, `RawContent (String)`<br>Shared Kernel writes: `sources` 表 CRUD、`chapters` TOC entries |
| **Ubiquitous Language** | BookSource, ScrapeRule, RuleAlt, Accessor, Scraper, Emulation (Chrome 131), RawContent, ChapterMeta |
| **Aggregates** | `BookSource` (root, identity = `bookSourceUrl`) |
| **Domain Events** | `BookSourceImported`, `BookSourceEnabled`, `BookSourceDisabled`, `NovelInfoFetched`, `TocFetched`, `ChapterContentFetched` |
| **Commands** | `ImportBookSource`, `EnableBookSource`, `DisableBookSource`, `FetchNovelInfo`, `FetchToc`, `FetchChapterContent` |
| **Open Questions** | Catalog 是否升 stateful（自帶 DB connection）— 等 multi-process 需求；`Novel` 雙用是否拆出 `CatalogNovelDraft` 獨立 type — 等 Library 端有不同欄位需求 |

### 3.2 Library

| 欄位 | 內容 |
|---|---|
| **Purpose** | 維護使用者書架（哪些書、各書狀態、章節快取、閱讀進度） |
| **Strategic Classification** | **Core**（核心領域 — 使用者真正 own 的資料） |
| **Domain Roles** | Execution（管理 shelf 生命週期）+ Audit（追蹤閱讀歷史） |
| **Inbound** | `AddNovel(book_url, source)`, `ListShelf()`, `GetNovel(id)`, `ReplaceToc(novel_id, chapters)`, `SaveChapterContent(novel_id, idx, content)`, `GetChapter(novel_id, idx)`, `SaveProgress(novel_id, idx, scroll)`, `GetProgress(novel_id)` |
| **Outbound** | Shared Kernel: `sources` 表（供 Catalog 借用）+ `chapters.content` column<br>對 Backup: 提供 read-only state snapshot via existing `Storage` API |
| **Ubiquitous Language** | ShelfEntry（目前以 `Novel` 表示）, Chapter, TocEntry, ChapterContentCache, ReadingProgress, ScrollOffset |
| **Aggregates** | `ShelfEntry` (root, identity = `book_url`)；`Chapter` 為 ShelfEntry 內的 entity（identity = `(novel_id, idx)`） |
| **Domain Events** | `NovelAddedToShelf`, `TocReplaced`, `ChapterCached`, `ProgressAdvanced`, `ProgressRolledBack` |
| **Commands** | `AddNovelToShelf`, `ReplaceToc`, `SaveChapterCache`, `SaveProgress` |
| **Open Questions** | Reading 何時拆獨立 context（觸發 = annotation / highlight / 多 session / 閱讀統計）；`Novel` 是否拆 `ShelfEntry + BookSnapshot`（觸發 = `added_at` 真的被讀 / 不同 lifecycle 需求） |

### 3.3 Backup

| 欄位 | 內容 |
|---|---|
| **Purpose** | Library 狀態能跨機器移動（export → 傳輸 → restore） |
| **Strategic Classification** | Generic |
| **Domain Roles** | Adherence（確保 Library state 可復原） |
| **Inbound** | `ExportSnapshot(path)`, `ImportSnapshot(path)`, `RunBackup()` |
| **Outbound** | Conformist 順從 `Storage::upsert_novel/save_progress`；ACL `BackedUpNovel ↔ Novel` 在 `import_from`<br>對外 IO: 寫檔案 (local) 或 PUT (webdav)；未來 native Google Drive |
| **Ubiquitous Language** | Snapshot, Backend (local / webdav), Receipt, RetentionPolicy (`keep`) |
| **Aggregates** | `Snapshot` (versioned, identity = filename + version) |
| **Domain Events** | `SnapshotExported`, `SnapshotImported`, `BackupPushed`, `BackupPruned` |
| **Commands** | `ExportSnapshot`, `ImportSnapshot`, `RunBackup`, `PruneOldBackups` |
| **Open Questions** | Native Google Drive backend 何時加（目前由 Drive Desktop + local 路徑代理）；webdav PROPFIND prune 是否要做 |

### 3.4 Presentation

| 欄位 | 內容 |
|---|---|
| **Purpose** | 翻譯人類意圖 ↔ 其他 context（CLI subcommand + TUI keypress） |
| **Strategic Classification** | Generic |
| **Domain Roles** | Gateway |
| **Inbound** | CLI subcommand（透過 clap）、TUI keypress（透過 crossterm） |
| **Outbound** | 呼 Catalog facade（Search/Fetch*）、Library facade（Add/List/Get/Save*）、Backup facade（Export/Import/Run）<br>**對外 PL（給 plugin layer）**：CLI subcommand grammar 是 stable contract，`.claude/skills/` 的三個 skill 都靠它運作 |
| **Ubiquitous Language** | Subcommand, ConfigKey, ReaderPane, ChapterCursor, ScrollState |
| **Aggregates** | 無純 domain aggregate。`ReaderApp` 是 UI session aggregate（包含目前 ChapterCursor 與 scroll offset — 此屬 Reading session state，未拆出 Reading context 前暫居此處） |
| **Domain Events** | `KeyPressed`, `PaneFocused`, `ProgressDisplayed` |
| **Commands** | 純被動 — 接 user input、不主動發 command 給其他 context（由 facade 層調度） |
| **Open Questions** | Reading 拆出後 `ReaderApp` 怎麼瘦身 — 大部分 reading session state 會搬走 |

---

## 4. File-to-Context × Layer Mapping

當前 `src/` → 重構後對應：

| 當前檔案 | 目的地 context | 目的地 layer | 備註 |
|---|---|---|---|
| `src/main.rs` | (root) | wiring | 模組宣告，不變 |
| `src/cli.rs` :: `Cli` / `Cmd` struct | Presentation | **action** | clap parse |
| `src/cli.rs` :: `run()` match arms | Presentation | **application facade** | 每個 match arm 是一個 use case，按 subcommand 拆檔到 `presentation/handlers/*.rs` |
| `src/source/mod.rs` :: `BookSource` 等 | Catalog | **service**（同時是 Shared Kernel 的 data type） | |
| `src/source/rule.rs` :: parse_rule / apply_* | Catalog | **service** | DSL 引擎 |
| `src/scraper.rs` :: `Scraper` | Catalog | **service** | HTTP + 套規則 |
| `src/scraper.rs` :: `resolve()` / `normalize_paragraphs()` | (any) | **utils** | 等第二個 caller 出現再搬 |
| `src/storage.rs` | 拆分 | **DAO** | 按 context 拆：`sources` writes ← `catalog/dao.rs`；`novels` / `chapters.content` / `progress` ← `library/dao.rs`；TOC writes (`chapters.idx/name/url`) ← Shared Kernel（兩 dao 都可呼） |
| `src/models.rs` :: `Novel` | Catalog + Library 雙用 | type | 暫不拆；觸發條件見 Library Open Q |
| `src/models.rs` :: `SearchHit` | Catalog | **PL type** | |
| `src/models.rs` :: `ChapterMeta` / `Chapter` | Shared Kernel | type | |
| `src/models.rs` :: `ReadProgress` | Library | type | |
| `src/backup.rs` :: `run_backup` | Backup | **facade** | |
| `src/backup.rs` :: `push_local` / `push_webdav` / `prune_local` | Backup | **service** | |
| `src/backup.rs` :: `build_backup` / `export_to` / `import_from` | Backup | **service**（含 ACL mapping） | |
| `src/config.rs` | (infra) | infra | 跨 context 共用，不歸任何 domain |
| `src/reader.rs` :: `App` struct + event_loop | Presentation | **facade**（ReaderApp）+ **service**（render） | Reading session state pending split |

---

## 5. Refactor Roadmap

每步先寫 then test, 且**保持中間態可編譯可跑**。

### Step 1: Backup（依賴最少；唯一介面 `Storage` 已穩定）

- 建立 `src/backup/{mod.rs, dao.rs, service.rs, facade.rs}`
- `backup/dao.rs`：thin wrapper of `Storage::upsert_novel / save_progress / list_novels / get_progress`（暫時 direct import `crate::storage::Storage`）
- `backup/service.rs`：`push_local / push_webdav / prune_local`
- `backup/facade.rs`：`run_backup / export_to / import_from`
- **不引入** `RestoreShelfFromBackup` command object（Conformist）
- 驗證：`cargo test` 全綠 + `cargo run -- backup` 端到端跑 + 看 Drive 同步檔還在

**為何排第 1**：依賴最少（只依賴 Storage public API），介面已穩定（CLI 不變），fail 不影響其他 context

### Step 2: Catalog（拆出後才知道 Library 對外要露什麼介面）

- 建立 `src/catalog/{mod.rs, service/{rule.rs, source.rs, scraper.rs}, dao.rs, facade.rs}`
- 搬：`source/mod.rs → catalog/service/source.rs`、`source/rule.rs → catalog/service/rule.rs`、`scraper.rs → catalog/service/scraper.rs`
- `catalog/dao.rs`：直接 import `crate::storage`，呼 `save_source / list_sources / get_source`；TOC writes 也在這（Shared Kernel）
- `catalog/facade.rs`：`search_with_source / fetch_novel_info / sync_toc`（cli.rs 的 Search/Add/Sync handler 將呼這層）
- 註明：sources / chapters 是 Shared Kernel，DAO 內顯式註解「此表 Library DAO 也會寫」
- 驗證：`cargo run -- source list / search / add / sync`

**為何排第 2**：Library 排第 3 是因為它的對外介面定義要看前兩步真實用到什麼

### Step 3: Library（介面定義依賴前兩步回饋）

- 建立 `src/library/{mod.rs, service/{shelf.rs, reading.rs}, dao.rs, facade.rs}`
- `library/service/shelf.rs`：純 domain logic + invariants（如「TOC sync 不破壞 progress」「ChapterCache 必須有對應 TOC entry」）
- `library/service/reading.rs`：progress 計算邏輯（scroll bounds、chapter advance）
- `library/dao.rs`：`novels / chapters.content / progress` 表 CRUD
- `library/facade.rs`：use case 編排
- 處理 storage row ↔ type mapping refactor（schema 不變，但 row → struct 的 (de)serialize 點要連動）
- 驗證：`cargo run -- shelf / read / tui`（含 progress save/load 來回）

**為何排第 3**：被依賴最深；前兩步穩定後再定型；mapping refactor 最容易撞 row level 細節

### Step 4: Presentation（純 adapter，收尾）

- 建立 `src/presentation/{mod.rs, handlers/, reader.rs}`
- `cli.rs run()` 各 match arm 拆檔到 `presentation/handlers/{source.rs, search.rs, shelf.rs, sync.rs, read.rs, tui.rs, config.rs, export.rs, import.rs, backup.rs}`
- `reader.rs → presentation/reader.rs`，保留 `ReaderApp`（標註 Reading session state pending split）
- 驗證：跑遍所有 CLI subcommand + 開 TUI 操作一輪

**為何排第 4**：純 adapter；其他 context 都穩定後變動最小

---

## 6. Open Questions

| # | 問題 | 觸發條件 / Owner | 為何能延後 |
|---|---|---|---|
| OQ-1 | Catalog 是否升 stateful（自帶 DB connection / process） | multi-process 部署需求 / Catalog as library 給其他 binary | 目前單 process，CLI 走 facade，無 isolation 需求 |
| OQ-2 | Reading 拆獨立 context | 加 annotation / highlight / 多 session / 閱讀統計任一 | 目前 reading 是「pointer + cache」，無獨立生命週期 |
| OQ-3 | Plugin 升等內部 context | novel-looker 為 plugin 提供非 CLI API（IPC / library mode） | 目前 plugin 透過 CLI 用，是外部 consumer |
| OQ-4 | `utils/` 第一個成員是誰 | 第二個 caller 出現（候選：resolve / normalize_paragraphs / backup_filename / safe_filename） | YAGNI，目前都單 caller |
| OQ-5 | DAO 從 `storage.rs` 怎麼拆 | Step 2 開始實作時依「實際呼叫頻率」決定 | 按 context (catalog/dao + library/dao) vs 按 SQL semantic (read/write) vs 集中保留共用 DAO crate — 由 `/analyze` 決定 |
| OQ-6 | `Novel` 拆 `ShelfEntry + BookSnapshot` | Library 需新欄位（如 `added_at` 真的被讀）或不同 lifecycle 需求 | 目前 `Novel` 雙用沒實質傷害；refactor 成本 > 收益 |
| OQ-7 | Webdav PROPFIND prune | 使用者抱怨 webdav 端堆積舊 snapshot | 目前只 local prune，webdav 端讓使用者手動管 |
| OQ-8 | Native Google Drive backend | 使用者沒裝 Drive Desktop 或要更高頻備份 | Drive Desktop + local 路徑已涵蓋多數情境 |

---

## 7. Glossary

- **Shared Kernel**：兩 context 共用的一段「核心模型 / 資料」。改動需要兩 context 都同意。DDD 認為 anti-pattern 邊緣，因為解耦最差，但在小系統 / 短期內優於過早抽象
- **PL (Published Language)**：兩 context 之間約定的對外型別格式，作為穩定契約
- **ACL (Anti-Corruption Layer)**：消費端把上游 PL 轉成自家 entity 的翻譯層，避免 upstream 概念污染下游
- **Conformist**：下游放棄抽象、直接順從上游介面（避免維護 mapping 的成本）
- **OHS (Open Host Service)**：上游提供 stable public API 給多個下游
- **Application Service**：跨 context 編排 use case 的 thin layer，**不屬於任何單一 context**；本專案中 = `cli.rs run()` match arms

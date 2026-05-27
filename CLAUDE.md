# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Rust terminal novel reader driven by **JSON book sources**. A new website
is added by writing a JSON file under `book-sources/`, not by editing Rust.
The CLI parses, scrapes, caches, and ships a ratatui TUI reader.

This repo will later be published as a Claude Code plugin (`.claude/` already
scaffolded with `parse-novel-site` and `add-to-shelf` skills).

## Build / run / test

```bash
# System deps (one-time on Debian/Ubuntu): cmake libclang-dev golang pkg-config
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --release
cargo test                            # all tests (no LIBCLANG needed once built)
cargo test catalog::service::rule::tests::parse_attr_and_replace        # one test
cargo run -- <subcommand>             # run CLI without installing
```

`LIBCLANG_PATH` is required because **wreq** depends on **BoringSSL** (via `boring`),
which compiles native C++ and needs bindgen → libclang. The path differs per distro;
on Ubuntu 24.04 it's `/usr/lib/llvm-18/lib`. First build takes ~2–3 min for
BoringSSL; subsequent builds are incremental.

CLI surface (run `cargo run -- help` for full list):

```
source import <path>     # 匯入 JSON 書源 (單檔或 JSON 陣列皆可)
source list
search <keyword> [--source <url>]
add --source <url> <book_url>
shelf
sync <novel_id>
read <novel_id> <chapter_idx>     # plain stdout
tui  <novel_id>                   # ratatui two-pane reader

config show
config set <key> <value>          # backup.backend / backup.keep
                                  # backup.local.path
                                  # backup.webdav.url / .username / .password
config path
export <path>                     # ad-hoc snapshot (shelf + progress, no content)
import <path>                     # restore from snapshot
backup                            # run export → push via configured backend
```

Local SQLite DB lives at `$XDG_DATA_HOME/novel-looker/novel-looker.db`
(via the `dirs` crate). **Not** in the repo.

## Architecture — DDD 4 context × 5 layer

Refactored at commit `e7c936a` into four bounded contexts. Each owns its
mod tree under `src/<context>/` and exposes a **facade** as its entry
contract. No flat top-level files for these domains any more.

```
src/
├── main.rs                 — clap entry; bootstraps AppContext
├── config.rs               — ~/.config/novel-looker/config.toml loader
├── utils/                  — cross-context helpers (URL resolve, …)
│
├── catalog/                — “how to extract” + extraction engine
│   ├── mod.rs              — PL: BookSource, SearchHit
│   ├── facade.rs           — save_source / list_sources / fetch_novel_info / sync_toc / fetch_chapter_content
│   ├── dao.rs              — Shared-Kernel writes (sources table, chapters.{idx,name,url})
│   └── service/            — source.rs · rule.rs · scraper.rs (pure domain, no SQL)
│
├── library/                — shelf · TOC · content cache · progress
│   ├── mod.rs              — PL: Novel, ChapterMeta, Chapter, ReadProgress
│   ├── facade.rs           — add_novel / list_shelf / get_chapter / save_chapter_content / save_progress …
│   ├── dao.rs              — owns the SQLite Connection (sole rusqlite entry point for Library)
│   └── service/shelf.rs    — invariants placeholder
│
├── backup/                 — Conformist of Library (4-layer, no dao)
│   ├── mod.rs
│   ├── facade.rs           — run_backup (export → push), BackupReceipt
│   └── service/            — snapshot.rs (export_to / import_from) · transport.rs (push_local / push_webdav)
│
└── presentation/           — CLI + TUI
    ├── mod.rs              — AppContext { db, scraper, config }
    ├── cli.rs              — Cli / Cmd enums + dispatch to handlers
    ├── handlers/*.rs       — one file per subcommand; composes catalog + library facades
    └── reader.rs           — ratatui two-pane TUI
```

### Layering rules — five layers, vertical per context

| Layer | Where | Rule |
|---|---|---|
| **mod.rs (PL)** | `<ctx>/mod.rs` | Only Published-Language types + sub-mod declarations. No logic. |
| **facade** | `<ctx>/facade.rs` | One fn per use case; called by presentation handlers. **Facades do not call other contexts’ facades** (sole exception: backup→library Conformist). |
| **service** | `<ctx>/service/*.rs` | Pure domain logic. **MUST NOT import rusqlite or any `dao` module.** |
| **dao** | `<ctx>/dao.rs` | The **only** rusqlite entry point for that context. Borrow contract: `&LibraryDb` for SELECT, `&mut LibraryDb` for INSERT/UPDATE/transaction. |
| **handlers (presentation)** | `presentation/handlers/<cmd>.rs` | Where cross-context use cases compose (e.g. `add` = `catalog::fetch_novel_info` + `library::add_novel`). Format CLI output here; no business logic. |

Backup is intentionally **4-layer** — it has no `dao.rs` and reaches data
exclusively through `library::facade` (`LibraryDbHandle` is re-exported
from `library::facade` so `backup` never imports `library::dao`).

### Shared Kernel between Catalog and Library

The `sources` table and the `chapters` table’s `idx / name / url` columns
are written by **Catalog DAO**. `chapters.content` is written by
**Library DAO**. Both DAOs share the single `LibraryDb` Connection
(catalog DAO methods take `&LibraryDb` / `&mut LibraryDb`). Modifying
the schema on either side requires checking the other side’s DAO.

### The rule DSL (`src/catalog/service/rule.rs`) — central design

Every selector in a `BookSource` is a string in this grammar:

```
<css_selector>[@<accessor>][##<regex>##<replacement>]
```

- `accessor`: `text` (default) | `html` | `outerHtml` | any HTML attribute name
- `||` joins fallback alternatives; first non-empty wins
- `&` as the selector is **intended** to mean "the current element itself"
  but is **currently broken in `extract_within`**: `Selector::parse("&")` is
  called before the `alt.selector == "&"` self-check at `rule.rs:148`, so
  any `&`-rule errors with `EmptySelector` before the override runs. Until
  fixed, structure `ruleToc` to select the wrapper element (e.g. `> li`)
  and put `a@text` / `a@href` in `chapterName` / `chapterUrl` so child
  selectors do the work.

The rule engine exposes four entry points consumed by `scraper.rs`:
- `select_nodes(doc, rule)` — element list from a document (for `bookList`, `chapterList`)
- `select_within(elem, rule)` — element list within an element  *(currently unused — kept for skills/future readers; dead-code warning expected)*
- `extract_doc(doc, rule)` → `Option<String>` — single value from document
- `extract_within(elem, rule)` → `Option<String>` — single value within an element
- `extract_all_doc(doc, rule)` → `Vec<String>` — all matches (used by `fetch_content` for multi-`<p>` content rules)

The `||` fallback and `&`-self handling are easy to break — touch
`rule.rs` with care and re-run `cargo test catalog::service::rule::tests`
(4 tests today).

### BookSource shape (`src/catalog/service/source.rs`)

Four rule groups mirror the four scraping stages:

| Group | Fields | Used by |
|---|---|---|
| `ruleSearch` | `url`, `bookList`, `name`, `author`, `bookUrl`, `kind`, `intro` | `Scraper::search` |
| `ruleBookInfo` | `name`, `author`, `intro`, `coverUrl`, `tocUrl` | `Scraper::fetch_info` |
| `ruleToc` | `chapterList`, `chapterName`, `chapterUrl`, `nextTocUrl` | `Scraper::fetch_toc` |
| `ruleContent` | `content`, `title`, `nextContentUrl`, `replaceRegex` | `Scraper::fetch_content` |

`ruleSearch.url` substitutes `{{key}}` (and the legacy `searchKey` literal)
with the URL-encoded keyword.

### Scraper invariants (`src/catalog/service/scraper.rs`)

- HTTP client is `wreq::Client` with `Emulation::Chrome131` — sends real
  Chrome JA3 / JA4 / HTTP-2 fingerprints so Cloudflare-protected sites
  (uukanshu.cc etc.) don't 403 us at the TLS layer. Do **not** swap back to
  `reqwest` without keeping an equivalent impersonation layer.
- All relative URLs are resolved against the page's *final* URL (after redirects);
  use `resp.uri()` (not `.url()` — wreq's API). The URL-join helper lives in
  `src/utils/url.rs::resolve` — share it with future scrapers, don’t re-inline.
- `fetch_content` post-processes HTML through `normalize_paragraphs` — `<br>` /
  `</p>` become real newlines, entities decode, remaining tags strip;
  uses `extract_all_doc` (NOT `extract_doc`) because a content rule typically
  matches many `<p>` elements.
- Headers JSON from `BookSource.header` is applied to every request when present;
  invalid JSON is silently ignored.

### Storage — Library DAO + Catalog DAO

Four tables: `sources`, `novels`, `chapters`, `progress`.

- `LibraryDb` (in `library/dao.rs`) owns the `Connection`; `catalog/dao.rs`
  borrows it via `&LibraryDb` / `&mut LibraryDb`.
- `novels.book_url` is the natural key (UPSERT on conflict in `library::dao::upsert_novel`).
- `catalog::dao::replace_toc` runs in a transaction: `DELETE FROM chapters WHERE
  novel_id=?` then `INSERT … content=NULL` for each chapter. **Caveat: this nukes
  cached content.** If you want re-sync to preserve `content` for stable URLs,
  convert to UPSERT keyed on `(novel_id, idx)` or merge old `content` by URL
  before reinsert. (An earlier CLAUDE.md claimed preservation; the code doesn’t.)
- `chapters.idx` is the literal `enumerate()` index from `fetch_toc`, **not** a
  dense 0..N-1 sequence. Any node that `select_nodes` returned but `fetch_toc`
  skipped (no `href` → `continue`) leaves a hole at that idx and shifts every
  following chapter by one. Example: czbooks's `<li class="volume">正文卷</li>`
  is the first match, gets `i=0`, is skipped because it has no `<a>`, and the
  real 第1章 ends up at `idx=1`. To set a reading progress by chapter label,
  query `chapters` first — don't assume `第N章 → idx=N-1`.
- `library::dao::save_chapter_content` mutates the row in-place — safe between syncs.
- DB path: `$XDG_DATA_HOME/novel-looker/novel-looker.db` (resolved via `dirs`).

### Presentation — handlers compose facades

Cross-context use cases live in `src/presentation/handlers/<cmd>.rs`, not
in either facade. Examples:

| Command | Handler composition |
|---|---|
| `add`  | `catalog::facade::get_source` → `catalog::facade::fetch_novel_info` → `library::facade::add_novel` |
| `search` | iterates **all** `enabled` sources when `--source` is omitted; per-source error is printed and the next source still runs |
| `sync` | `library::facade::get_novel` → `catalog::facade::get_source` → `catalog::facade::sync_toc` |
| `read` | `library::facade::get_chapter` (cache hit) **or** `catalog::facade::fetch_chapter_content` → `library::facade::save_chapter_content` (cache miss) |
| `backup` | `backup::facade::run_backup(&db, &config)` — backup itself is the use case |

There is **no** subcommand to re-bind an existing shelf row to a different
`book_source_url` (i.e. no “換源”). `novels.book_source_url` is set once by
`library::dao::upsert_novel` at `add` time; to switch source you currently
have to delete the row from `novels` and re-`add` from the new URL — which
also drops cached TOC + content + progress via the `ON DELETE CASCADE`.

When adding a new subcommand, add the variant to `presentation/cli.rs::Cmd`,
create `presentation/handlers/<name>.rs`, route it in `cli::run`, and
keep all formatting/printing in the handler.

### TUI screen 在 layer rule 的歸屬

`presentation/handlers/tui/{menu, shelf, search, reader, switch_source}.rs` 每個
screen 等同一個 handler — 屬於 presentation 層的 cross-context use case 組合點。
Screen 可同時 import `catalog::facade` + `library::facade`（與 CLI handler 同層、同
權限）。layer invariant 不變：`grep -nE "use crate::(catalog|library)::facade"
src/catalog src/library` 仍應零命中 — catalog/library 內部不互呼 facade。

入口分流：`Cli.cmd: Option<Cmd>`；無參數 → `handlers::menu::handle` 進 TUI 主菜單
（`App::new_with_menu`）；`tui <id>` 走 `App::new_with_direct_reader`；其他既有
子命令維持。

### TUI Reader (`src/presentation/handlers/tui/reader.rs`)

Two-pane layout: TOC（30% 寬，可 `t` 縮為 0）+ content。Reader 採 **Eager 3-chapter
buffer + mode state machine** 架構（2026-05-27 重寫，commit `7c54bb...` 之後）：

**Buffer model**

- state 持 `ChapterBuffer { combined_text, prev/curr/next_chapter_idx, prev_end_row, curr_end_row }`
  — prev + curr + next 三章拼成單一 String，分隔 `"\n\n"`
- `init_buffer(curr_idx, ...)` 用於 reader 開啟（任一章 fetch 失敗 → 整個 reader 開不起來，
  上層帶 toast 提示）；`rebuild_buffer(curr_idx, ...) -> Result<_, RebuildError>` 用於
  reader 內跳章（`CurrFailed` → 保留舊 buffer + toast；`PartialDegraded(buf)` → 用降級
  buffer + toast）
- 第 0 章 / 最後章自動降級為 2 章 buffer（prev / next 為 None、對應 end_row 為 0）
- scroll 越 prev 開頭 / 越 next 結尾 → 自動 `rebuild_buffer(viewport_top_chapter)`；
  prev=None / next=None 時鎖在邊界、不再 rebuild（避免遞迴）
- 跳 TOC = 完整 rebuild + scroll = `new_buf.prev_end_row`（viewport 對齊新 curr 開頭）

**Mode state machine**

```
ReaderMode::Normal
ReaderMode::Filter { query: String, filtered_indices: Vec<usize>, selected: usize }
```

- Normal 模式：既有 j/k/Tab/Space/PgUp/Dn/g/G/q/m/n/p 鍵 + 新加 `t` (TOC toggle) / `/` (進 Filter)
- Filter 模式：`Char(c)` append query 並重算 fuzzy filter、`Backspace` 刪 / `Esc` 退 / `Enter`
  跳到 `chapters[filtered_indices[selected]]` / `j`/`k` 移 `selected`
- **重要規則**：Filter 模式下 *所有* printable Char 一律 append query（包含 `t` / `q` / `/`），
  否則 query 永遠輸不進這些字元
- `/` 進入 Filter 時強制 `toc_collapsed = false`（filter 看 TOC 列才有意義）；退出不還原
- Filter 模式 Mouse click 等同 Enter 路徑

**Scraper injection seam (test only)**

- `pub(crate) trait ScraperLike { async fn fetch_chapter_content(src, url) -> Result<String> }`
- production `impl ScraperLike for catalog::Scraper`
- `init_buffer<S>` / `rebuild_buffer<S>` 兩支 fn 都 generic over `S: ScraperLike` —
  UT 直接傳 `MockScraper { by_url, panic_on_call }` 注入；handle_event 透過
  `apply_*` wrapper 把 production scraper 從 `&mut AppContext` 解出來
- 配 `ratatui::backend::TestBackend` 做 draw round-trip 驗證（如 `toc_width_cached`
  在 draw 後更新）

**純函數 (Testability Seam 3)**

抽成 free fn 方便 UT 直接呼：
- `viewport_top_chapter(buffer, scroll) -> i64` — scroll 落在哪段（prev / curr / next）
- `progress_text(buffer, scroll, total) -> String` — 底端進度條「第 X 章 / 共 N 章 (Y%)」，
  X = `viewport_top_chapter + 1`、Y 用 u64 算
- `hit_test_pane(column, toc_width) -> Pane` — mouse hit 在 TOC 還 Content
- `hit_test_toc_row(row, list_offset, items_count) -> Option<usize>` —
  row → list idx（含 None 防越界），list_offset = 1（top border）
- `apply_fuzzy_filter(query, chapters) -> Vec<usize>` —
  SkimMatcherV2 對 `ChapterMeta.name` 跑分、score desc

**Mouse 行為**

| Event | Normal mode | Filter mode |
|---|---|---|
| Wheel @ TOC pane | 跳上/下章（rebuild） | `selected ± 1` in filtered_indices |
| Wheel @ Content pane | scroll ±3 行 | (no-op) |
| Wheel @ collapsed TOC | 視為 content pane | (同) |
| Click(Left) @ TOC row | `apply_jump_to(row idx)` | 跳到 `chapters[filtered_indices[row]]` + 退 Filter |
| Click(Left) @ Content | no-op | no-op |

**Toast 機制**

reuse shelf-delete 引入的 `TOAST_TTL = 3 秒` + `toast_active()` pattern：
`toast: Option<String>` + `toast_expires_at: Option<Instant>`、draw 走 `toast_active()`
getter（用 `Option::filter` 判過期）。rebuild 失敗時設 toast 顯示 3 秒。

**Keybindings 一覽**

```
Normal:
  j / k         上/下一章（跳章 + rebuild）
  n / p         同 j / k（保留）
  J / K         scroll ± content_area_h
  Space / PgDn  scroll +content_area_h
  PgUp          scroll -content_area_h
  g / G         buffer 開頭 / 結尾
  Tab           focus TOC / Content 切換
  t             TOC pane 縮合 / 展開 (30% ↔ 0%)
  /             進 Filter mode
  q / m         save progress 後退出
  Mouse Wheel   pane-aware：TOC = 跳章 / Content = ±3 行
  Mouse Click   點 TOC row 跳該章

Filter:
  Char          append query + 重算 filter
  Backspace     pop query（空 → no-op）
  j / k         filtered_indices selected ± 1
  Enter         跳 chapters[filtered_indices[selected]] + 退 Filter（空 filter → no-op）
  Esc           退 Filter（buffer / scroll / current 不變）
  Mouse Wheel   TOC pane = selected ± 1（不改 reader.current）/ Content = no-op
  Mouse Click   點 row = 跳 + 退 Filter
```

**Trait sig（infra 重寫）**

```rust
#[async_trait::async_trait(?Send)]
pub trait Screen {
    async fn handle_event(&mut self, event: Event, ctx: &mut AppContext) -> Transition;
    fn draw(&mut self, f: &mut Frame, ctx: &AppContext);
}
```

`run_loop` 內部 `run_inner<E: EventSource, T: TerminalLike>` — 兩個 trait 是 UT 注入
testable seam，production 用 `CrosstermEventSource` + `RawTerm`。run_loop forward
`Event::Key | Event::Mouse`；`Resize / Paste / FocusGained` 走 `continue`。

Progress (`chapter_index` + `scroll_offset`) 在每次跳章與 quit 時 save 到
`library::facade::save_progress`。

### Backup / config (`src/backup/`, `src/config.rs`)

- Config lives at `$XDG_CONFIG_HOME/novel-looker/config.toml` (separate from the
  data DB — config travels via dotfiles, data via `backup`).
- Snapshot JSON (`backup/service/snapshot.rs`) deliberately **omits chapter content**
  — re-sync is cheap (~1s per book), but losing reading progress is the actual user pain.
- `backup.backend` is currently `local` or `webdav`; Google Drive is reachable
  via `local` + Drive Desktop's synced folder (no OAuth needed). Adding a
  native Google backend would mean a new `backup::service::transport::push_google`
  alongside `push_local` / `push_webdav` — **not** a new top-level module.
- WebDAV password is read from `$NOVEL_LOOKER_WEBDAV_PASS` first, then from
  the config file. Prefer the env var — config files end up in screenshots and
  git diffs.

## Claude plugin

本 repo 是一個 Claude Code plugin。manifest 在 `.claude-plugin/plugin.json`
（Claude Code CLI 期待的標準路徑）：

```bash
# 驗證 manifest
claude plugin validate .

# session-only 載入（不 install）
cd /home/vakarve/projects/rust/novel-looker
claude --plugin-dir .

# 或永久 install（從 local path 當 marketplace）
claude plugin marketplace add /home/vakarve/projects/rust/novel-looker
claude plugin install novel-looker
```

skills（在 `.claude/skills/`，由 plugin.json 引用）：
- `parse-novel-site/SKILL.md` — 給 URL，自動分析 DOM 結構，產 `book-sources/<site>.json`
  （rule DSL 描述要與 `src/catalog/service/rule.rs` 同步）
- `add-to-shelf/SKILL.md` — 包 `add → sync → tui` 流程
- `legado-converter/SKILL.md` — 把 Legado / 閱讀 3.0 JSON 書源轉本專案格式

When changing the rule grammar or `BookSource` fields, update
`parse-novel-site/SKILL.md` so generated sources stay valid.

## What not to touch unprompted

- The `bookSourceUrl`-as-PK contract in `library::dao` (sources table) — `source list`
  and pattern-matching in `add-to-shelf` both depend on it being a stable URL.
- Field names in `BookSource` (and `#[serde(rename = ...)]` mappings) — JSON
  sources in the wild use this exact camelCase.
- Cross-context layering: never let `service/*.rs` files import `rusqlite` or
  any `dao` module — the bounded-context split depends on this. Backup must
  not import `library::dao` directly; go through `library::facade`.
- Dead-code helpers in `catalog/service/rule.rs` (`select_within`) and the
  `BackupReceipt.filename` field — kept for skills / future readers / debug;
  the two `dead_code` warnings are expected and should stay.

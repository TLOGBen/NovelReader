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
- `&` as the selector means "the current element itself"
  (used inside a list iteration, e.g. `chapterName: "&@text"`)

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
- `library::dao::save_chapter_content` mutates the row in-place — safe between syncs.
- DB path: `$XDG_DATA_HOME/novel-looker/novel-looker.db` (resolved via `dirs`).

### Presentation — handlers compose facades

Cross-context use cases live in `src/presentation/handlers/<cmd>.rs`, not
in either facade. Examples:

| Command | Handler composition |
|---|---|
| `add`  | `catalog::facade::get_source` → `catalog::facade::fetch_novel_info` → `library::facade::add_novel` |
| `sync` | `library::facade::get_novel` → `catalog::facade::get_source` → `catalog::facade::sync_toc` |
| `read` | `library::facade::get_chapter` (cache hit) **or** `catalog::facade::fetch_chapter_content` → `library::facade::save_chapter_content` (cache miss) |
| `backup` | `backup::facade::run_backup(&db, &config)` — backup itself is the use case |

When adding a new subcommand, add the variant to `presentation/cli.rs::Cmd`,
create `presentation/handlers/<name>.rs`, route it in `cli::run`, and
keep all formatting/printing in the handler.

### TUI (`src/presentation/reader.rs`)

Two-pane layout. The fetch is awaited **inline** inside the event loop
(blocks UI for ~1–3s). If you make fetching truly async, add a tokio mpsc
channel and `tokio::select!` over `event::poll` + channel `recv`.

Keybindings: `j/k` chapters | `J/K`/`Space`/`PgUp/Dn` scroll | `n/p` next/prev
chapter | `Tab` switch focus | `g/G` head/tail | `q` quit.

Progress (`chapter_index` + `scroll_offset`) is saved on every chapter change
and on quit via `library::facade::save_progress`.

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

## Claude plugin scaffolding

- `.claude/plugin.json` — manifest, references the skills
- `.claude/skills/parse-novel-site/SKILL.md` — given a URL, produce a
  validated `book-sources/<site>.json`; the skill spec defines the validation
  commands and the rule DSL (keep that section in sync with
  `src/catalog/service/rule.rs`)
- `.claude/skills/add-to-shelf/SKILL.md` — wraps `add → sync → tui`
- `.claude/skills/legado-converter/SKILL.md` — port Legado / 閱讀 3.0 JSON sources

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

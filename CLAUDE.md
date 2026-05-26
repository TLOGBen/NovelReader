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
cargo test --package novel-looker rule::tests::parse_attr_and_replace   # one test
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

## Architecture

Data flows in one direction:

```
book-sources/*.json
    ↓ source::BookSource (serde)
    ↓
Storage (SQLite)  ←→  Scraper (wreq + scraper)
    ↓                         ↑
CLI (clap)  ──────────────────┘
    ↓
reader.rs (ratatui)         backup.rs ← config.rs
```

### Layering rules

Keep code in three layers + a utils pool. New code MUST honor this; when
modifying existing files, gradually move logic toward the right layer rather
than tearing down working modules in one shot.

| Layer | Where | Responsibility |
|---|---|---|
| **action** | `cli.rs` (`Cmd` variants, `run()` match arms) | Parse CLI args, format human output, delegate to facade. **No business logic.** |
| **facade** | Top-level functions in each domain module (e.g. `backup::run_backup`, `backup::export_to`) | Orchestrate a use case end-to-end across multiple services. One call per intent. |
| **service** | Module-internal impls (e.g. `Storage::*`, `Scraper::*`, `backup::push_local/push_webdav`, `source::rule::*`) | Single-responsibility primitives. Don't reach across domains. |
| **utils** | (planned: `utils/mod.rs`) | Pure helpers reused across domains (URL resolve, paragraph normalize, filename sanitize). |

Right now utility helpers like `resolve` / `normalize_paragraphs` (in `scraper.rs`)
and `backup_filename` (in `backup.rs`) still live next to their first caller.
Move them to a shared `utils` module on next refactor pass once a second caller
appears.

### The rule DSL (`src/source/rule.rs`) — central design

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
- `select_within(elem, rule)` — element list within an element
- `extract_doc(doc, rule)` → `Option<String>` — single value from document
- `extract_within(elem, rule)` → `Option<String>` — single value within an element

The `||` fallback and `&`-self handling are easy to break — touch
`rule.rs` with care and re-run its unit tests.

### BookSource shape (`src/source/mod.rs`)

Four rule groups mirror the four scraping stages:

| Group | Fields | Used by |
|---|---|---|
| `ruleSearch` | `url`, `bookList`, `name`, `author`, `bookUrl`, `kind`, `intro` | `Scraper::search` |
| `ruleBookInfo` | `name`, `author`, `intro`, `coverUrl`, `tocUrl` | `Scraper::fetch_info` |
| `ruleToc` | `chapterList`, `chapterName`, `chapterUrl`, `nextTocUrl` | `Scraper::fetch_toc` |
| `ruleContent` | `content`, `title`, `nextContentUrl`, `replaceRegex` | `Scraper::fetch_content` |

`ruleSearch.url` substitutes `{{key}}` (and the legacy `searchKey` literal)
with the URL-encoded keyword.

### Scraper invariants (`src/scraper.rs`)

- HTTP client is `wreq::Client` with `Emulation::Chrome131` — sends real
  Chrome JA3 / JA4 / HTTP-2 fingerprints so Cloudflare-protected sites
  (uukanshu.cc etc.) don't 403 us at the TLS layer. Do **not** swap back to
  `reqwest` without keeping an equivalent impersonation layer.
- All relative URLs are resolved against the page's *final* URL (after redirects);
  use `resp.uri()` (not `.url()` — wreq's API)
- `fetch_content` post-processes HTML through `normalize_paragraphs` — `<br>` /
  `</p>` become real newlines, entities decode, remaining tags strip;
  uses `extract_all_doc` (NOT `extract_doc`) because a content rule typically
  matches many `<p>` elements
- Headers JSON from `BookSource.header` is applied to every request when present;
  invalid JSON is silently ignored

### Storage (`src/storage.rs`)

Four tables: `sources`, `novels`, `chapters`, `progress`.
- `novels.book_url` is the natural key (UPSERT on conflict)
- `replace_toc` runs in a transaction (chapters are wiped + reinserted)
- `save_chapter_content` mutates the row in-place — TOC sync does **not**
  drop cached content if URLs are stable

### TUI (`src/reader.rs`)

Two-pane layout. The fetch is awaited **inline** inside the event loop
(blocks UI for ~1–3s). If you make fetching truly async, add a tokio mpsc
channel and `tokio::select!` over `event::poll` + channel `recv`.

Keybindings: `j/k` chapters | `J/K`/`Space`/`PgUp/Dn` scroll | `n/p` next/prev
chapter | `Tab` switch focus | `g/G` head/tail | `q` quit.

Progress (`chapter_index` + `scroll_offset`) is saved on every chapter change
and on quit.

## Claude plugin scaffolding

- `.claude/plugin.json` — manifest, references the two skills
- `.claude/skills/parse-novel-site/SKILL.md` — given a URL, produce a
  validated `book-sources/<site>.json`; the skill spec defines the validation
  commands and the rule DSL (keep that section in sync with `rule.rs`)
- `.claude/skills/add-to-shelf/SKILL.md` — wraps `add → sync → tui`

When changing the rule grammar or `BookSource` fields, update
`parse-novel-site/SKILL.md` so generated sources stay valid.

### Backup / config (`src/backup.rs`, `src/config.rs`)

- Config lives at `$XDG_CONFIG_HOME/novel-looker/config.toml` (separate from the
  data DB — config travels via dotfiles, data via `backup`)
- Snapshot JSON deliberately **omits chapter content** — re-sync is cheap (~1s
  per book), but losing reading progress is the actual user pain
- `backup.backend` is currently `local` or `webdav`; Google Drive is reachable
  via `local` + Drive Desktop's synced folder (no OAuth needed). Adding a
  native Google backend would require a separate `backup::push_google` service
  alongside the existing local/webdav ones
- WebDAV password is read from `$NOVEL_LOOKER_WEBDAV_PASS` first, then from
  the config file. Prefer the env var — config files end up in screenshots and
  git diffs

## What not to touch unprompted

- The `bookSourceUrl`-as-PK contract in `storage.rs` — `source list` and
  pattern-matching in `add-to-shelf` both depend on it being a stable URL
- Field names in `BookSource` (and `#[serde(rename = ...)]` mappings) — JSON
  sources in the wild use this exact camelCase
- Dead-code helpers in `rule.rs` (`select_within`, `extract_all_doc`) — kept
  for the reader/skills to use; warnings are expected

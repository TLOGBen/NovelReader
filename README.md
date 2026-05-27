# novel-looker

Rust 寫的終端機小說閱讀器，採「資料驅動書源」+ DDD 架構。
每個小說網站 = 一份 JSON 書源；不必為新網站重新編譯。
也是一個 Claude Code plugin（內附 3 個 skills 自動產書源）。

## 功能

- **資料驅動書源**：四組 CSS 規則（`ruleSearch / ruleBookInfo / ruleToc / ruleContent`），
  迷你 DSL：`selector@attr##regex##replace`，`||` 串 fallback
- **TLS 指紋偽裝**：`wreq` + BoringSSL 模擬 Chrome 131 JA3/JA4，過 Cloudflare 不 403
- **SQLite 本地書架**：書 / 章節 / 進度全本地存 `$XDG_DATA_HOME/novel-looker/`
- **ratatui TUI 閱讀器**（2026-05-27 大改寫）：
  - **Eager 3-chapter buffer** — prev/curr/next 三章拼一個 String，跨章邊界滾動視覺無斷裂
  - **TOC toggle** — `t` 鍵兩態切（30% ↔ 0%），縮起騰出閱讀空間
  - **Fuzzy filter** — `/` 進 filter mode、邊打邊 fuzzy 過濾 TOC、Enter 跳該章
  - **滑鼠支援** — 滾輪 pane-aware（TOC = 跳章、Content = ±3 行）、左鍵點 TOC 跳章
- **書架刪除** — TUI 內 `d` 鍵 modal 確認、CLI `remove <id>`（含 `--yes` skip）
- **WebDAV / local 備份** — 匯出 / 匯入快照（不含章節內容、re-sync 即可）

## 安裝

需要編譯 BoringSSL（給 wreq 用）：

```bash
sudo apt install -y cmake libclang-dev golang pkg-config   # 一次性 system deps
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo install --path . --force
```

首次編譯 ~2-3 分鐘（BoringSSL）。後續增量編譯秒回。
路徑 `LIBCLANG_PATH` 依發行版調整（Ubuntu 24.04 = `/usr/lib/llvm-18/lib`）。

## 使用

```bash
# 匯入書源
novel-looker source import examples/gutenberg.json
novel-looker source list

# 搜尋（無 --source 遍歷所有 enabled 書源）
novel-looker search "alice"
novel-looker search "誅仙" --source https://czbooks.net/

# 加入書架
novel-looker add --source https://czbooks.net https://czbooks.net/n/xxxxx

# 同步章節 + 開讀
novel-looker shelf
novel-looker sync 1
novel-looker tui 1                # 直接進 reader
novel-looker                      # 無參數進 TUI 主菜單

# 進度匯出 / 還原
novel-looker config set backup.backend local
novel-looker config set backup.local.path ~/Dropbox/novel-looker.json
novel-looker backup               # 跑當前 backend
novel-looker export ~/snapshot.json
novel-looker import ~/snapshot.json

# 刪書
novel-looker remove 5             # 互動確認
novel-looker remove 5 --yes       # 直接刪
```

## TUI Reader 鍵盤 / 滑鼠

```
Normal mode
  j / k         上 / 下一章（跳章 + rebuild 3-chapter buffer）
  n / p         同 j / k（保留舊習慣）
  J / K         scroll ± content_area_h
  Space / PgDn  scroll +content_area_h
  PgUp          scroll -content_area_h
  g / G         buffer 開頭 / 結尾
  Tab           focus TOC / Content 切換
  t             TOC pane 縮 / 展（30% ↔ 0%）
  /             進 Filter mode（fuzzy 搜章節）
  q / m         save progress 後退出
  Mouse Wheel @ TOC      跳上 / 下章
  Mouse Wheel @ Content  scroll ± 3 行
  Mouse Click @ TOC row  跳該章

Filter mode
  Char          append 到 query，邊打邊過濾
  Backspace     刪 query（空 query no-op）
  j / k         移動 filtered_indices selected
  Enter         跳到 highlighted 章 + 退 Filter
  Esc           退 Filter（buffer 不變）
  Mouse Wheel @ TOC      移動 selected（不跳章）
  Mouse Click @ row      跳該章 + 退 Filter
```

## 書源規則語法

```
<css_selector>[@<accessor>][##<regex>##<replacement>]
```

- `accessor`：`text`（預設）/ `html` / `outerHtml` / `<attr name>`
- 多個規則用 `||` 串接，取第一個非空結果
- `&` 代表「當前節點本身」（**目前 bug：在 `extract_within` 不可用**，
  詳見 `src/catalog/service/rule.rs`）

範例：

```json
{
  "ruleSearch": {
    "url":      "https://example.com/search?q={{key}}",
    "bookList": "li.result",
    "name":     ".title@text",
    "author":   ".author@text",
    "bookUrl":  "a.link@href"
  }
}
```

## 架構（DDD 4 contexts × 5 layers）

```
src/
├── main.rs                — clap 入口
├── config.rs              — ~/.config/novel-looker/config.toml
│
├── catalog/               — 「怎麼抓」+ 抓取引擎（書源 / 規則 / scraper）
│   ├── facade.rs          — 對外契約：fetch_novel_info / sync_toc / fetch_chapter_content
│   ├── dao.rs             — shared kernel writes (sources + chapters.{idx,name,url})
│   └── service/           — source.rs / rule.rs / scraper.rs
│
├── library/               — 書架 + TOC + 內容快取 + 進度
│   ├── facade.rs          — add_novel / list_chapters / save_chapter_content / save_progress …
│   └── dao.rs             — owns SQLite Connection
│
├── backup/                — local / WebDAV 備份（Library 的 Conformist）
│
└── presentation/          — CLI + TUI
    ├── cli.rs             — clap 子命令枚舉
    ├── handlers/*.rs      — 每個子命令一檔；組合 catalog + library facades
    └── handlers/tui/*.rs  — TUI screens (menu / shelf / reader / search / switch_source / delete_confirm)
```

每個 bounded context 是 5 層（PL / facade / service / dao / 由 presentation handlers 跨層組合）。
catalog / library / backup 內部**不互呼 facade**；唯一例外是 backup → library（Conformist）。

## Claude Code Plugin

本 repo 本身就是 plugin。`.claude-plugin/plugin.json` 是 manifest，`.claude/skills/` 是 skills：

```bash
# Session-only 載入
cd /path/to/novel-looker
claude --plugin-dir .

# 永久 install（把 repo 當 local marketplace）
claude plugin marketplace add /path/to/novel-looker
claude plugin install novel-looker

# 驗證 manifest
claude plugin validate .
```

3 個 skills：

- **parse-novel-site** — 給網站 URL，自動分析 DOM 結構、產出可用的書源 JSON 並驗證
- **add-to-shelf** — 包 `add → sync → tui` 流程，把書從 URL 加進書架直接讀
- **legado-converter** — 把 Legado / 閱讀 3.0 的 JSON 書源轉本專案格式

## Data 位置

- DB: `$XDG_DATA_HOME/novel-looker/novel-looker.db`（不在 repo 內，跟著 dotfiles 走）
- Config: `$XDG_CONFIG_HOME/novel-looker/config.toml`
- WebDAV 密碼：優先讀 `$NOVEL_LOOKER_WEBDAV_PASS` env var，再讀 config（避免進 git diff）

## TODO

- [ ] 並行 fetch（reader Eager buffer prev+next）— 目前序列 fetch，cache hit < 10ms 可接受
- [ ] resize / paste / focus event 處理（trait 已收 `Event`，行為待補）
- [ ] 換源（switch source 從現有書找同名書、模糊 match 章節保進度）
- [ ] EPUB 匯出
- [ ] CJK 字寬正確化（目前 ratatui 對混排中英可能對齊抖）

## License

MIT

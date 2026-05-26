# novel-looker

一個 Rust 寫的終端機小說閱讀器，採「資料驅動書源」架構。
每個小說網站 = 一份 JSON 書源，不必為新網站重新編譯。

## 功能

- JSON 書源（四組規則：`ruleSearch / ruleBookInfo / ruleToc / ruleContent`）
- 迷你 CSS 規則 DSL：`selector@attr##regex##replace`，`||` 串 fallback
- SQLite 本地書架 + 章節快取 + 閱讀進度
- ratatui 兩欄 TUI 閱讀器（章節列表 | 內文）

## 安裝

需要編譯 BoringSSL（給 wreq 用，能模擬 Chrome TLS/JA3 指紋過 Cloudflare）。

```bash
# Debian/Ubuntu 系統 deps（一次性）
sudo apt install -y cmake libclang-dev golang pkg-config

# 編譯（LIBCLANG_PATH 指向 libclang.so 位置）
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --release
```

首次編譯 ~2-3 分鐘（BoringSSL）。後續增量編譯秒回。

## 使用

```bash
# 匯入書源
novel-looker source import examples/gutenberg.json
novel-looker source list

# 搜尋
novel-looker search "alice"

# 加入書架
novel-looker add --source https://www.gutenberg.org https://www.gutenberg.org/ebooks/11

# 同步章節 + 開讀
novel-looker shelf
novel-looker sync 1
novel-looker tui 1
```

## 書源規則語法

```
<css_selector>[@<accessor>][##<regex>##<replacement>]
```

- `accessor`：`text`（預設）/ `html` / `outerHtml` / `<attr name>`
- 多個規則用 `||` 串接，取第一個非空結果
- `&` 代表「當前節點本身」（在子規則中使用）

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

## 架構

```
src/
  main.rs      入口
  cli.rs       clap 指令
  source/      書源結構 + 規則引擎
  scraper.rs   HTTP + 套規則
  storage.rs   SQLite
  reader.rs    ratatui TUI
  models.rs    Novel / Chapter / Progress
```

## Claude Plugin（規劃中）

本 repo 之後會以 Claude Code plugin 形式發佈。已預留：

- `.claude/plugin.json` — 插件清單
- `.claude/skills/parse-novel-site/` — 給 URL，自動分析 DOM 產生書源 JSON
- `.claude/skills/add-to-shelf/` — 把書加進書架 + 同步 + 開讀（包 CLI）
- `book-sources/` — 累積收的書源 JSON

## TODO

- [ ] 並行多書源搜尋
- [ ] JS 渲染頁面支援（headless browser）
- [ ] 書源批次匯入工具
- [ ] EPUB 匯出
- [ ] 完善 plugin skills + 加入 slash commands

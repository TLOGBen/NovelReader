---
name: parse-novel-site
description: 給一個小說網站，分析其 DOM 結構並產出可被 novel-looker CLI 直接匯入的 JSON 書源。觸發詞：「解析這個小說網站」「幫我寫書源」「新增書源」「parse novel site」。輸入需要至少一個搜尋頁 URL 或書籍詳情頁 URL。
---

# parse-novel-site

把一個小說網站變成一份能被 `novel-looker` 抓的 JSON 書源。

## 啟動條件

- 使用者給出網域或具體 URL
- 觸發詞包含「解析這個小說網站」「幫我寫書源」「新增書源」「parse novel site」

## 流程

### 1. 收集樣本 URL

跟使用者確認以下至少其中一個（越多越準）：

- **搜尋頁**：能用關鍵字搜的網址（URL 內含搜尋字串，標記出來變數位置 → 轉成 `{{key}}`）
- **書籍詳情頁**：任何一本書的介紹頁（含書名、作者、章節入口）
- **章節列表頁**：完整目錄頁（若與詳情頁不同）
- **章節內文頁**：任何一章的內文

如果只給了網域，**請主動搜尋一個關鍵字、或抓首頁找熱門書連結**作為樣本。

### 2. 抓取 + 分析 DOM

對每個樣本 URL：

1. 用 WebFetch 或 Bash `curl -sL -A "Mozilla/5.0" <url>` 取 HTML
2. 觀察結構，找出：
   - **搜尋頁**：結果列表的共同容器選擇器（如 `li.book`），以及子節點的 title / author / book_url
   - **詳情頁**：書名、作者、簡介、封面、目錄入口
   - **目錄頁**：章節列表容器、章節 name + URL
   - **內文頁**：內文容器選擇器
3. 對 CSS class 命名混淆嚴重的站，優先用結構選擇器 (`tag > tag`)，避開 framework 隨機 hash

### 3. 規則語法（必須遵守）

```
<css_selector>[@<accessor>][##<regex>##<replacement>]
```

- `accessor`：`text` (預設) / `html` / `outerHtml` / `<attr 名>` (如 `href`、`src`)
- `||` 連接 fallback：`.title || h1`
- 子規則內可用 `&` 表「當前節點本身」（例如 `chapterName: "&@text"`、`chapterUrl: "&@href"`）

### 4. 輸出 JSON

寫入 `book-sources/<site_name>.json`，欄位完整範本：

```json
{
  "bookSourceUrl": "https://example.com",
  "bookSourceName": "示例小說網",
  "bookSourceGroup": "中文/玄幻",
  "enabled": true,
  "bookUrlPattern": "https?://example\\.com/book/\\d+",
  "ruleSearch": {
    "url": "https://example.com/search?q={{key}}",
    "bookList": "...",
    "name": "...",
    "author": "...",
    "bookUrl": "...@href"
  },
  "ruleBookInfo": {
    "name": "...",
    "author": "...",
    "intro": "...",
    "coverUrl": "...@src",
    "tocUrl": "...@href"
  },
  "ruleToc": {
    "chapterList": "...",
    "chapterName": "&@text",
    "chapterUrl": "&@href"
  },
  "ruleContent": {
    "content": "..."
  }
}
```

### 5. 驗證

```bash
cargo run -- source import book-sources/<site_name>.json
cargo run -- search "<keyword>"        # 確認 ruleSearch
cargo run -- add --source <bookSourceUrl> <book_url>   # 確認 ruleBookInfo
cargo run -- sync <novel_id>           # 確認 ruleToc
cargo run -- read <novel_id> 0         # 確認 ruleContent
```

任一步失敗 → 回頭重抓對應頁面、修選擇器、再跑。**不要在沒驗證的情況下宣稱完成**。

### 6. 交付

- 列出最終 JSON 路徑
- 列出驗證指令的輸出片段（前 3 筆搜尋 / 章節數 / 內文前 200 字）
- 若有任何規則用了不穩的選擇器（如隨機 hash class），標註 `TODO: brittle selector`

## 不要做的事

- 不要爬付費/登入牆內容（無 cookie 支援）
- 不要對同站短時間發超過 5 次請求（加上 sleep 1）
- 不要把 cookie / token 寫進 JSON

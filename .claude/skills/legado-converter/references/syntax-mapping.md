# Legado → novel-looker 規則語法對照

## 頂層欄位

| Legado | novel-looker | 備註 |
|---|---|---|
| `bookSourceUrl` | `bookSourceUrl` | 直搬 |
| `bookSourceName` | `bookSourceName` | 直搬 |
| `bookSourceGroup` | `bookSourceGroup` | 直搬 |
| `enabled` | `enabled` | 預設 true |
| `bookUrlPattern` | `bookUrlPattern` | 直搬，不驗證 |
| `header` | `header` | 直搬（JSON 字串） |
| `searchUrl` | `ruleSearch.url` | **移到 nested**；若已有 `ruleSearch.url` 則不覆蓋 |
| `bookSourceComment`, `bookSourceType`, `customOrder`, `enabledCookieJar`, `enabledExplore`, `exploreUrl`, `lastUpdateTime`, `loginUrl`, `respondTime`, `weight`, `ruleExplore` | — | **丟棄**；novel-looker 沒對應功能 |

## 規則字串語法

每條規則字串先按 `||` 分割成 alts，逐 alt 處理：

```
alt = [@css:]<selector>[@<accessor>][##<regex>[##<replacement>]]
```

### 直接相容

| Legado | novel-looker | 範例 |
|---|---|---|
| `selector@attr` | `selector@attr` | `.title@text`, `a@href` |
| `||` fallback | `||` fallback | `.a || h1@text` |
| `##regex##rep` | `##regex##rep` | `.t##^第.+?章\s*##` |
| `##regex`（無替換） | `##regex##`（空替換） | 視為移除符合處 |

### 需要轉換

| Legado | novel-looker | 規則 |
|---|---|---|
| `@css:selector` | `selector` | 去掉 `@css:` 前綴 |
| `selector@@inner@attr` | `selector inner@attr` | `@@` → 空格（CSS descendant） |
| `text` (bare) | `&@text` | bare accessor → self accessor |
| `html` (bare) | `&@html` | 同上 |
| `outerHtml` (bare) | `&@outerHtml` | 同上 |

### 不支援（會被腳本警告並跳過）

| Legado | 為什麼 |
|---|---|
| `//meta[@property=...]` 等 XPath | 我們的 scraper 只有 CSS |
| `@xpath:...` | 同上 |
| `@js:...`, `<js>...</js>` | 沒嵌 JS engine |
| `@put{name:rule}`, `@get{name}` | 沒實作變數系統 |
| `{{book.bookUrl}}` 等執行期變數 | 同上；只認 `{{key}}` / `{{page}}` |
| URL 後接 `,{"method":"POST",...}` | scraper 只發 GET |
| `@hetu:...`, `@json:...` | 沒對應引擎 |

XPath 偵測：`^//` 或 `^/html` 或 `/@\w+` 或 `[@\w+=`。

Legado 變數偵測：`{{[a-zA-Z_]\w*(\.\w+)*}}`，例外清單為 `key` / `page` / `searchKey`。

## ruleSearch / ruleBookInfo / ruleToc / ruleContent 子欄位

只保留 novel-looker 認識的欄位，其他丟棄（serde 不嚴格，不丟也行，但保持輸出乾淨）。

| Group | 保留欄位 |
|---|---|
| `ruleSearch` | `url`, `bookList`, `name`, `author`, `kind`, `intro`, `bookUrl` |
| `ruleBookInfo` | `name`, `author`, `kind`, `intro`, `coverUrl`, `tocUrl` |
| `ruleToc` | `chapterList`, `chapterName`, `chapterUrl`, `nextTocUrl` |
| `ruleContent` | `content`, `title`, `nextContentUrl`, `replaceRegex` |

被丟掉的常見欄位：`coverUrl`/`lastChapter` in ruleSearch（搜尋結果不顯示封面）、`wordCount`/`lastChapter` in ruleBookInfo（書架不顯示字數）。

## 內文規則的特殊語意

`ruleContent.content` 通常會 match 多個 `<p>`。novel-looker 的 scraper
在 `fetch_content` 內用 `extract_all_doc` 把所有 match 串起（`\n\n` 分隔），
不像其他規則只取第一個 match。轉換器**不需要**為此做特別處理——
規則字串本身不變。

## 邊界案例

### `ruleSearch.url` 沒有 `{{key}}`

通常代表是 GET form / 純 POST，搜尋不會動。轉換器不警告，但匯入後 search
會直接拿 raw URL 撈頁面，結果固定（如首頁）。**驗證階段就會發現**。

### `header` 是 JSON 字串

直搬，scraper 會在 `fetch()` 內 parse 並套用每個 header。若 header 含
Legado 變數（`{{cookie.xxx}}`）會 parse 失敗——scraper 會吞錯誤、不套
header，但其他 rule 仍能跑。

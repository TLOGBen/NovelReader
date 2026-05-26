---
name: legado-converter
description: 把 Legado / 閱讀 3.0 格式的書源 JSON 轉成 novel-looker 可匯入的格式，並驗證能跑。輸入可以是檔案、yckceo 等聚合站的 URL，或剪貼上來的 JSON 字串。觸發詞：「轉 legado 書源」「import legado source」「閱讀 3.0 書源」「yckceo 書源」「shuyuan json」。**遇到使用者提到 Legado / 閱讀 3.0 / yckceo / 別的 reader app 書源時，主動使用本 skill**——不要傻傻地手動翻譯欄位。
---

# legado-converter

Legado / 閱讀 3.0 的書源 JSON → novel-looker JSON。重要的是：**轉完一定要實測**，否則使用者裝了垃圾書源會直接 crash。

## 何時觸發

- 使用者貼出 Legado 書源 JSON、yckceo 連結（`yckceo.com/yuedu/shuyuan/...`）、或提到「閱讀 3.0」「reader」書源
- 使用者說「轉這個書源」「這個能在 novel-looker 用嗎」

## 為何要寫成 skill 而非當下手寫

兩個格式有 6 處系統性差異：頂層 `searchUrl` vs 巢狀 `ruleSearch.url`、`@css:` 前綴、`@@` 子選擇器、POST 設定附在 URL 後、XPath/JS/變數（**不支援**，必須偵測並警告）、bare accessor (`text` → `&@text`)。腳本一次到位，比逐欄位想清楚快很多。

## 流程

### 1. 載入來源

腳本支援三種輸入：

```bash
# 本地檔案
python3 .claude/skills/legado-converter/scripts/convert.py path/to/legado.json

# 直接吃 yckceo 詳情頁 URL（自動從 <pre> 抽 JSON）
python3 .claude/skills/legado-converter/scripts/convert.py \
  "https://www.yckceo.com/yuedu/shuyuan/content/id/7321.html"

# 從剪貼板（透過 stdin）
pbpaste | python3 .claude/skills/legado-converter/scripts/convert.py -
```

輸出寫到 `book-sources/<bookSourceName>.json`（被 `.gitignore` 擋，不會誤推）。
用 `--stdout` 印到 stdout 而不寫檔。
用 `--out-dir <path>` 改寫入位置。

### 2. 看警告

腳本對下列規則會**跳過該 alt 並警告**——這些代表書源在 novel-looker 上會缺欄位或抓不到內容：

| 警告 | 意思 | 影響 |
|---|---|---|
| `XPath ...` | 規則是 `//meta[@property=...]` 之類 | 該欄位變空（如書名 / 作者抓不到） |
| `@js: / <js>` | Legado 用 JS 計算 | 同上 |
| `@put / @get` | Legado 變數宣告 / 取用 | 同上 |
| `runtime-variable {{book.xxx}}` | 需要 book 上下文才能解 | 同上 |
| `POST/headers config not supported` | URL 後接 `,{"method":"POST",...}` | 變 GET，搜尋通常失敗 |

> 若 `ruleBookInfo.name` / `ruleToc.chapterList` / `ruleContent.content` 任何一個被警告掉 → **這份書源在 novel-looker 上不能用**。告訴使用者並建議換書源。

### 3. 匯入 + 三段驗證

每一份新轉的書源都要跑這三步，**不要跳過**：

```bash
cd ~/projects/rust/novel-looker      # 切到專案
cargo build --bin novel-looker 2>/dev/null
BIN=./target/debug/novel-looker

$BIN source import book-sources/<name>.json
# 1) 搜尋驗證 ruleSearch
$BIN search "<熱門關鍵字>" --source "<bookSourceUrl>"
# 拿第一筆 book URL...
# 2) 加書驗證 ruleBookInfo
$BIN add --source "<bookSourceUrl>" "<book_url>"
# 印出 #<id>，書名作者非 "Unknown" 才算通過
# 3) 同步 + 讀章驗證 ruleToc + ruleContent
$BIN sync <id>
$BIN read <id> 0 | head -50
# 章節文字 > 5 行才算內文 rule 通過
```

任一步失敗 → 看以下對照表處理。

### 4. 失敗對照

| 症狀 | 通常原因 | 處置 |
|---|---|---|
| `search` 印出 `(no results)` | 站點封爬 / `bookList` 選擇器漂掉 | 開 Chrome 看搜尋頁，更新 `ruleSearch.bookList` |
| `add` 印 `Unknown / -` | `ruleBookInfo.name` / `author` 被 XPath 警告掉 | 找新書源；或手動補 CSS 版規則 |
| `sync 0 章` | `ruleToc.chapterList` 對不上目錄頁 DOM | 開 Chrome 看目錄頁，改 chapterList 選擇器 |
| `read` 只有 1-2 行 | `ruleContent.content` 選擇器太窄（只配一個 `<p>`） | 已修：scraper 用 `extract_all`，更新 binary 即可 |
| `read` 完全空 | content 規則用了 XPath / JS | 換書源 |

### 5. 交付給使用者

成功的話，回報：
- 書源檔案路徑 (`book-sources/<name>.json`)
- 章節數 / 內文預覽前幾段（證明真的通了）
- 警告數量 + 缺失的欄位（如「coverUrl 抓不到，因為原書源用 XPath」）

## 規則語法對照

詳見 `references/syntax-mapping.md`。

## 邊界

- 不處理 RSS 書源（`ruleArticle`）
- 不處理音訊 / 漫畫（`bookSourceType != 0`）
- 不下載批次合集再爬——使用者該手動給 URL
- `bookUrlPattern` 直接搬，不驗證正則正確性

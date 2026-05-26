---
name: add-to-shelf
description: 把一本小說加進 novel-looker 書架、同步章節、開啟 TUI 閱讀。觸發詞：「加進書架」「我想看這本」「把這本加到 novel-looker」。輸入需要小說詳情頁的 URL。
---

# add-to-shelf

一個指令把書收進書架並準備好讀。

## 啟動條件

- 使用者貼出小說詳情頁 URL
- 觸發詞：「加進書架」「我想看這本」「add to shelf」

## 流程

> 預設 `BIN=./target/debug/novel-looker`（或 release 版）。若使用者給的是**章節 URL**
> （如 `.../book/21940/12662515.html`），先用 regex 推回**詳情頁 URL**：通常砍掉
> 最後一段 `\d+\.html` 即可。確認推出來的 URL 跟 `bookUrlPattern` 對得上。

1. 從 URL 推測屬於哪個書源：
   ```bash
   $BIN source list
   ```
   比對每個 `bookUrlPattern`（regex），找到 match 的 `bookSourceUrl`。若無 match，
   先呼叫 `parse-novel-site` skill 產生新書源再回來。

2. 加入書架：
   ```bash
   $BIN add --source <book_source_url> <book_url>
   ```
   印出 `#<novel_id>` + 書名 + 作者。

3. 同步章節：
   ```bash
   $BIN sync <novel_id>
   ```
   印出「✓ 同步 N 章」。

4. 詢問使用者要不要立刻 `$BIN tui <novel_id>`、列出指定章節 `$BIN read <id> <idx>`，
   或先加進去之後再看。若使用者本來就在看某一章，幫他算出 `idx`：
   ```bash
   sqlite3 ~/.local/share/novel-looker/novel-looker.db \
     "SELECT idx, name FROM chapters WHERE novel_id=<id> AND url LIKE '%<chapter_slug>%'"
   ```

## 驗證

- `$BIN shelf` 能看到新書、書名作者不是 `Unknown / -`
- `$BIN sync` 章節數 > 0

## 失敗處理

| 症狀 | 通常原因 | 處置 |
|---|---|---|
| `add` 印 `Unknown / -` | `ruleBookInfo.name` / `author` 選擇器漂掉 | 開 Chrome 看詳情頁，更新 selector |
| `sync 0 章` | `ruleToc.chapterList` 對不上目錄頁 DOM | 同上看目錄頁 |
| `read` 完全空 | `ruleContent.content` 對不上內文容器 | 同上看章節頁 |
| `add` / `sync` hang 很久後失敗 | 站點封 IP 或要登入 | 改用 VPN / 該書源加 cookie header |

> ⚠ Cloudflare TLS 攔截**不再是問題**——scraper 用 wreq + Chrome 指紋
> 已自動處理。若 add 還是 fail，**就是規則或網路**，不是 TLS。

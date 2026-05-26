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

1. 從 URL 推測屬於哪個書源：
   ```bash
   cargo run -- source list
   ```
   比對 `bookUrlPattern`，找到匹配的 `bookSourceUrl`。若無匹配，先呼叫 `parse-novel-site` skill 產生書源。

2. 加入書架：
   ```bash
   cargo run -- add --source <book_source_url> <book_url>
   ```
   印出 `#<novel_id>`。

3. 同步章節：
   ```bash
   cargo run -- sync <novel_id>
   ```

4. 詢問使用者要不要立刻 `tui <novel_id>`，還是只先加進去。

## 驗證

- `cargo run -- shelf` 能看到新書
- `cargo run -- sync` 印出的章節數 > 0

## 失敗處理

- 若 `add` 拿到 `name=Unknown` → ruleBookInfo 失準，跳回 parse-novel-site 修書源
- 若 `sync` 章節數 = 0 → ruleToc 失準，同上

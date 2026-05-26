# Setup — Preflight ceremony for /execute

**所有 task 開始前必須完成本檔列出的事項**。/execute session 第一動作即執行此 ceremony；中途若切 shell session、清過 `target/`、或重啟，回頭重跑相關段。

---

## 1. 環境變數

每個 shell session 開頭執行：

```bash
export LIBCLANG_PATH=/usr/lib/llvm-18/lib
```

理由：`wreq` → `boring-sys2` → `bindgen` → `libclang` 編譯鏈缺此變數會 fatal link error，且錯誤訊息冗長（「could not find libclang」+ build script 大量噴吐），/execute 容易誤判為「refactor 改壞了」回頭瞎查。

**驗證**：`echo $LIBCLANG_PATH` 印出 `/usr/lib/llvm-18/lib`。

---

## 2. Baseline ref（重構前 git HEAD）

```bash
git rev-parse HEAD > /tmp/refactor-base.sha
cat /tmp/refactor-base.sha   # 印出來確認非空
```

理由：plugin 不變 gate（REQ-006）與 wip/analyze 目錄不變 gate 都需要與「重構開始前」的狀態比對。`git diff --stat HEAD` 在中間有 commit 後比較對象會漂走。

---

## 3. Baseline DB 內容驗證 & 必要時補建

新 binary 重構後仍要能讀舊資料（C5 + REQ-004 Scen 4.3/4.5）。先確認 baseline DB 內容：

```bash
sqlite3 ~/.local/share/novel-looker/novel-looker.db <<'SQL'
SELECT count(*) AS novels FROM novels;
SELECT count(*) AS chapters FROM chapters;
SELECT count(*) AS progress FROM progress;
SELECT id, name, source_url FROM novels;
SQL
```

**期望最少狀態**（refactor 驗證需要的最少 fixture）：
- 至少 1 本書（任何書源都可，但有 uukanshu 超維術士最理想，因為章節數最多、能驗 Cloudflare bypass + 大 TOC）
- 該書有 sync 過的 TOC（chapters 表 > 0）
- 該書有 progress 一筆（progress 表 > 0）

**若 baseline 不足，於 task-shared 之前先補建**：

```bash
# 確認書源存在
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo run -- source list
# 若 uukanshu 不在
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo run -- source import book-sources/uukanshu.json

# 確認超維術士在書架
NID=$(sqlite3 ~/.local/share/novel-looker/novel-looker.db \
  "SELECT id FROM novels WHERE name='超維術士' LIMIT 1")
if [ -z "$NID" ]; then
  LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo run -- \
    add --source https://uukanshu.cc https://uukanshu.cc/book/21940/
  NID=$(sqlite3 ~/.local/share/novel-looker/novel-looker.db \
    "SELECT id FROM novels WHERE name='超維術士'")
fi
# 同步章節
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo run -- sync $NID
# 製造 progress（讀第 0 章）
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo run -- read $NID 0 > /dev/null
```

**驗證**：
```bash
sqlite3 ~/.local/share/novel-looker/novel-looker.db \
  "SELECT count(*) FROM novels; SELECT count(*) FROM chapters; SELECT count(*) FROM progress;"
# 三項都 > 0 即可
```

**取得 NID 變數供後續 task 使用**（每個 shell session 開頭重設）：

```bash
export NID=$(sqlite3 ~/.local/share/novel-looker/novel-looker.db \
  "SELECT id FROM novels WHERE name='超維術士' LIMIT 1")
echo "NID=$NID"   # 印出非空 integer
```

---

## 4. Baseline CLI grammar snapshot

REQ-003 Scen 3.1 要求 CLI grammar 不變。重構前先抓 baseline help text：

```bash
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker 2>/dev/null
./target/debug/novel-looker help > /tmp/help-baseline.txt
for cmd in source search add shelf sync read tui config export import backup; do
  ./target/debug/novel-looker $cmd --help > /tmp/help-$cmd-baseline.txt 2>&1
done
echo "baseline saved: /tmp/help-*-baseline.txt"
```

**最終驗證方式**（在 task-presentation-04）：
```bash
diff <(./target/debug/novel-looker help) /tmp/help-baseline.txt
# 對每個 subcommand 同樣 diff
```

---

## 5. Baseline cargo build 狀態

```bash
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo build --bin novel-looker 2>&1 | tee /tmp/build-baseline.log
grep -c "^warning:" /tmp/build-baseline.log
# 記住此數字（預期 2：select_within + extract_all_doc dead_code）
```

REQ-005「warning 數量 ≤ 基線」以此為基準。

---

## 6. 紀律約束

- **不執行 `cargo clean`**：首次 wreq + boring-sys2 編譯 ~2-3 分鐘，重複 clean 會把整條 pipeline 拉長到不可接受。若編譯出現詭異 link error，先 `cargo build` 重試或檢查 LIBCLANG_PATH，不清 target/
- **不直接 commit `.claude/{wip,analyze,skills}/`**：本目錄 + plugin 三目錄在整個重構期間不得被任何 task 寫入或 commit。可加 `.gitignore` 或在 commit 前 `git reset HEAD .claude/{skills,wip,analyze}/`
- **每個 task 結束都跑 `cargo build` 確認可編譯**：保持中間態可跑（design.md Verification 段）
- **不在 main 分支直接 refactor**：建議 `git checkout -b refactor/ddd-context-split` 後做，最終 PR 才合回

---

## 7. /execute 進場 checklist

開始第一個 task 前確認：

- [ ] `echo $LIBCLANG_PATH` 印 `/usr/lib/llvm-18/lib`
- [ ] `cat /tmp/refactor-base.sha` 印出 git sha
- [ ] `sqlite3 ... "SELECT count(*) FROM novels"` 印 > 0
- [ ] `echo $NID` 印整數
- [ ] `ls /tmp/help-baseline.txt` 存在
- [ ] `wc -l /tmp/build-baseline.log` 存在
- [ ] `git branch --show-current` 印出 refactor branch 名

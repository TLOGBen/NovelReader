# Confirm — Execute Session

session_start: 2026-05-26T07:05:00Z
spec_dir: .claude/analyze/2026-05-26-ddd-refactor/
classification: M

## 已讀取文件

| 檔案 | 讀取時間 |
|------|---------|
| goal.md | 2026-05-26T07:05:00Z |
| requirement.md | 2026-05-26T07:05:00Z |
| design.md | 2026-05-26T07:05:00Z |
| test.md | 2026-05-26T07:05:00Z |
| setup.md | 2026-05-26T07:05:00Z |
| task-shared.md | 2026-05-26T07:05:00Z |
| task-library.md | 2026-05-26T07:05:00Z |
| task-catalog.md | 2026-05-26T07:05:00Z |
| task-backup.md | 2026-05-26T07:05:00Z |
| task-presentation.md | 2026-05-26T07:05:00Z |

## DAG 分析

原始 DAG（依 `前置群組` 解析）：

| Frontier Level | Groups | 前置 |
|---------------|--------|-------|
| 0 | shared | 無 |
| 1 | library | shared |
| 2 | catalog, backup | library（並行候選） |
| 3 | presentation | library, catalog, backup |

Raw max frontier width: 2

### Pre-scan：file conflict 偵測

對 Level 2 並行候選（catalog + backup）做 `步驟` 段 file path 比對：

| 共用 file path | catalog 動作 | backup 動作 | 衝突嚴重度 |
|---|---|---|---|
| `src/main.rs` | catalog-02 移除 `mod scraper;` / 加 `mod catalog;` | backup-01 移除 root `mod backup;` 加 sub-dir `mod backup;` | line-level non-overlapping (auto-merge OK) |
| `src/storage.rs`（library 留的 alias） | catalog-03 移除 sources / TOC alias | backup-01 不直接動 storage.rs（透過 library::facade） | 無寫衝突 |
| 中間態 alias 鏈時序 | catalog 依賴 storage.rs alias 仍在 | backup 也依賴 library::facade 已建好 | **時序耦合**：alias 移除順序錯誤會編譯破 |

**決議**：序列化 catalog → backup → presentation（catalog 與 backup 不並行）。

理由：file 級非阻塞但時序耦合非阻塞。spec 設計的 alias 過渡策略（task-library-02 建 alias、task-presentation-02 刪 alias）依賴 catalog/backup 中間動作對 alias 的不變式維護；並行會增加 alias 鏈被破壞的窗口。serialize 換取簡單推理。

### 調整後 DAG

| Frontier Level | Groups | Notes |
|---------------|--------|-------|
| 0 | shared | 前置：無 |
| 1 | library | depends on shared |
| 2 | catalog | depends on library |
| 3 | backup | depends on library (serialized after catalog per pre-scan) |
| 4 | presentation | depends on all |

Effective max frontier width: 1
Classification: **M**
Parallel workflows: 1
Worktrees: none (main branch)

## 補充：使用者交接 context

從上下文 prompt 萃取的執行 guard rails（不可違反）：

- 必先讀 `setup.md` 並執行 preflight（LIBCLANG_PATH env / baseline ref / baseline DB / NID）
- 所有 cargo 指令前綴 `LIBCLANG_PATH=/usr/lib/llvm-18/lib`
- `.claude/skills/` 一字不動
- SQLite schema / CLI grammar / JSON / config.toml key 不變
- Backup 是 4 層（無 dao.rs）
- TRANSITION marker convention：所有過渡別名強制標 `// TRANSITION:`；最終 grep gate 為空
- 建議 fresh branch `refactor/ddd-context-split`

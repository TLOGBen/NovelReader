# Impl Checklist: backup

前置群組：library

## TASK-backup-01: 建立 backup/ 結構 + 搬 Snapshot type 與 ACL mapping

需求追溯：REQ-001 (Scen 1.1, 1.2, 1.3), REQ-002 (Scen 2.1, 2.2)

- [x] 目錄結構完整（**不含 dao.rs** — Backup 是 4 層 Conformist）
- [x] `backup/mod.rs` 開頭 doc comment：Conformist of Library 說明
- [x] `service/snapshot.rs` 含 Snapshot 相關 type + `build_backup(&LibraryDb)` + `export_to(&LibraryDb, &Path)` + `import_from(&mut LibraryDb, &Path)`
- [x] cargo build + test 通過
- [x] 舊資料 `backup` 與 `export` 功能性等價

Review 結果：advisory
備註：Strategy A 原子搬移完成，5 個新檔（mod / facade / service/{mod,snapshot,transport}），src/backup.rs 已刪。snapshot.rs 透過 `library::facade::LibraryDbHandle as LibraryDb` 引用 DB handle，未 import `library::dao`（Scen 2.2 嚴格達標）。snapshot.rs 的 storage call 全走 `library::facade::{list_shelf, get_progress, add_novel, save_progress}`（Conformist 表達正確）。Independent verify：cargo build + test 4 passed，2 warnings = baseline，grep `use rusqlite` / `use crate::library::dao` 在 service/ 內為 0 輸出。

---

## TASK-backup-02: transport + facade（Conformist，無 dao）

需求追溯：REQ-001 (Scen 1.2), REQ-002 (Scen 2.4), REQ-004 (Scen 4.4)

- [x] `service/transport.rs` 含 4 個 transport 函數
- [x] `facade.rs` 含 `run_backup(&mut LibraryDb, &Config) -> Result<BackupReceipt>`
- [x] **沒有** `src/backup/dao.rs`
- [x] `src/backup.rs`（root 舊檔）刪除
- [x] cargo build + test 通過
- [x] `cargo run -- backup` 印備份成功 + 檔案實際在 backup.local.path
- [x] `grep -rn "use rusqlite" src/backup/`：無輸出

Review 結果：advisory
備註：transport.rs 含 push_local / push_webdav / prune_local / backup_filename 4 函數，與 baseline 行為等價。run_backup 簽名實際為 `&LibraryDb`（非 `&mut`）— spec 寫 `&mut` 是保守，但 backup flow 純讀 DB（export_to → snapshot.rs 全部 read-only），`&LibraryDb` 在 borrow 規則上更精確、符合 design.md L187（SELECT-only 用 `&LibraryDb`）。cli.rs:246 配合 `&store` 呼叫。  Advisory（不影響 task acceptance）：(1) `backup/facade.rs:18` 直接 `use crate::library::dao::LibraryDb;` — mod.rs doc comment 自宣告「不允許 import `crate::library::dao::*`」，建議改為 `use crate::library::facade::LibraryDbHandle as LibraryDb;` 與 snapshot.rs 對齊，貫徹 Conformist；(2) `BackupReceipt.filename` field never read 產生新 dead_code warning（總數仍 = baseline 2）— 可改 `#[allow(dead_code)]` 或讓 cli.rs 印出檔名以點亮。兩點皆不影響 backup 正確性與驗收。

# Tasks: backup
**前置群組**：library（與 catalog 並行 OK，但若需 SearchHit / Scraper 不在範圍內，純粹 library 完成即可開工）

## TASK-backup-01: 建立 backup/ 結構 + 搬 Snapshot type 與 ACL mapping

**需求追溯**：REQ-001 (Scen 1.1, 1.2, 1.3), REQ-002 (Scen 2.1, 2.2)
**目標**：`src/backup/{mod.rs, facade.rs, service/{mod.rs, snapshot.rs, transport.rs}}` 建立（**4 層，無 dao.rs** — Backup 是 Library 的 Conformist，storage access 走 library::facade）；`Backup / BackedUpNovel / ProgressDump / ImportSummary / BackupReceipt` 等 type + `build_backup / export_to / import_from` 搬到 `service/snapshot.rs`。

**驗收標準**：
- [ ] 目錄結構完整（**不含** dao.rs）
- [ ] `backup/mod.rs` 開頭 doc comment：`//! Backup: Library 狀態跨機器移動。Conformist of Library — 順從 Library DAO 既有 mutation API。`
- [ ] `service/snapshot.rs` 含 Snapshot 相關 type + `build_backup(&LibraryDb) -> Result<Backup>` + `export_to(&LibraryDb, &Path) -> Result<usize>` + `import_from(&LibraryDb, &Path) -> Result<ImportSummary>`
- [ ] cargo build + test 通過
- [ ] 舊資料 `backup` 與 `export` 功能性等價

### 步驟

#### 建立 backup 模組
- [ ] `mkdir -p src/backup/service`
- [ ] `src/backup/mod.rs`：doc（含「Conformist of Library — 無自有 DAO，storage access via library::facade」）+ `pub mod facade; pub mod service;`（**無 dao**）
- [ ] `src/backup/service/mod.rs`：`pub mod snapshot; pub mod transport;`
- [ ] `src/main.rs`：移除 `mod backup;`（root level）→ 加 `mod backup;`（新指向 backup/ dir）

#### 搬 Snapshot type
- [ ] 從 `src/backup.rs` 剪下 `Backup / BackedUpNovel / ProgressDump / ImportSummary` 定義，貼到 `src/backup/service/snapshot.rs`
- [ ] 同時搬 `VERSION` const、`build_backup` / `export_to` / `import_from` 函數
- [ ] 改為接 `&LibraryDb` 而非 `&Storage`（如果 storage.rs 已刪則直接用 LibraryDb；否則暫時用 `crate::storage::Storage` 型別別名）
- [ ] **NOTE**：`build_backup` 與 `import_from` 內呼的 `store.list_novels / store.get_progress / store.upsert_novel / store.save_progress` 全部改為 `crate::library::facade::*` 對應函數，這是 Conformist 順從 Library facade 的具體實現

#### 驗證
- [ ] `cargo build`
- [ ] `cargo run -- export /tmp/test-export.json`：成功印「✓ 匯出 N 本書」

---

## TASK-backup-02: transport 層 + facade（Conformist，無 dao）

**需求追溯**：REQ-001 (Scen 1.2), REQ-002 (Scen 2.4), REQ-004 (Scen 4.4 — 備份流程等價)
**目標**：`push_local / push_webdav / prune_local / backup_filename` 搬到 `service/transport.rs`；`run_backup` 搬到 `facade.rs`。**Backup 不建 dao.rs**——`run_backup` 與 `import_from` 內所有 storage 操作呼 `crate::library::facade::*`（Conformist：Library facade 即是 Backup 對 storage 的唯一介面）。

**驗收標準**：
- [ ] `service/transport.rs` 含 4 個 transport-related 函數
- [ ] `facade.rs` 含 `run_backup(&LibraryDb, &Config) -> Result<BackupReceipt>`
- [ ] **沒有** `src/backup/dao.rs`（Conformist 設計刻意不建）
- [ ] `src/backup.rs`（root 舊檔）刪除
- [ ] cargo build + test 通過
- [ ] `cargo run -- backup`：印「✓ 備份 N 本書 [local] → ...」+ 檔案實際在 `backup.local.path`

### 步驟

#### 搬 transport
- [ ] 從 `src/backup.rs` 剪 `push_local / push_webdav / prune_local / backup_filename`，貼到 `src/backup/service/transport.rs`
- [ ] 函數 signature 保持不變（接 `&Path`, `&Config`, etc.）

#### 寫 facade
- [ ] `src/backup/facade.rs` 寫 `pub async fn run_backup(...) -> Result<BackupReceipt>`，內部呼 service::snapshot 與 service::transport

#### Conformist 直呼 Library facade（不建 dao）
- [ ] backup/facade.rs 內 `run_backup` 與 backup/service/snapshot.rs 內 `build_backup / import_from` 全部直呼 `crate::library::facade::*`
- [ ] backup/mod.rs doc comment 明確標：「Backup is 4-layer: no dao. Storage access via library::facade (Conformist)」

#### 移除舊檔
- [ ] `rm src/backup.rs`（注意：是 root 的舊 backup.rs，新的在 src/backup/ 目錄）
- [ ] 確認 `src/main.rs` 的 `mod backup;` 指向新目錄

#### 驗證
- [ ] `cargo build`，warning 不增加
- [ ] `cargo run -- backup`：印備份成功
- [ ] 檢查 `/mnt/g/我的雲端硬碟/novel-looker-backup/` 有新檔（時間戳是當下）
- [ ] `cargo run -- export /tmp/test.json`：成功
- [ ] `cargo run -- import /tmp/test.json`：成功（含進度）
- [ ] `grep -rn "use rusqlite" src/backup/`：無輸出（DAO 不直接接 SQL）
- [ ] `grep -rnE "use crate::(catalog|library)::facade" src/backup/facade.rs`：應**有**輸出（backup/facade.rs 確實呼 library facade，這是 Conformist 的具體實現；本檢查與 design.md「facade 不互呼」的差異是：handler 才是禁止互呼的點，backup/facade 作為 Conformist 是例外，記錄於 design.md NOTE）

//! Backup facade — orchestrates snapshot export + transport push.
//!
//! `run_backup` 是 `backup` 子命令的唯一入口。flow:
//!   1. `service::snapshot::export_to(db, tmp)` — 寫 snapshot JSON 至 tmp
//!   2. 根據 `config.backup.backend` 選擇 transport：local / webdav
//!   3. Best-effort 清理 tmp
//!   4. 回傳 [`BackupReceipt`]
//!
//! Conformist 註：本 facade 透過 `service::snapshot::export_to`
//! 間接呼到 `library::facade::list_shelf` / `get_progress`，
//! 不直接 import `library::facade`（snapshot.rs 已封裝該細節）。

use anyhow::Result;

use crate::backup::service::snapshot;
use crate::backup::service::transport;
use crate::config::Config;
use crate::library::facade::LibraryDbHandle as LibraryDb;

#[derive(Debug)]
pub struct BackupReceipt {
    pub destination: String,
    pub novels: usize,
    pub filename: String,
}

pub async fn run_backup(db: &LibraryDb, config: &Config) -> Result<BackupReceipt> {
    let filename = transport::backup_filename();
    let mut tmp = std::env::temp_dir();
    tmp.push(&filename);
    let count = snapshot::export_to(db, &tmp)?;

    let receipt = match config.backup.backend.as_str() {
        "local" => transport::push_local(&tmp, &filename, config)?,
        "webdav" => transport::push_webdav(&tmp, &filename, config).await?,
        other => anyhow::bail!("unknown backup backend: {other}"),
    };

    // Best-effort cleanup of tmp file.
    let _ = std::fs::remove_file(&tmp);

    Ok(BackupReceipt {
        destination: receipt,
        novels: count,
        filename,
    })
}

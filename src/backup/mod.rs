//! Backup: Library 狀態跨機器移動。Conformist of Library —
//! 順從 Library DAO 既有 mutation API。
//!
//! Bounded Context: Backup
//!
//! Outbound PL (對外發布之型別):
//! - [`facade::run_backup`]        — `backup` 子命令的入口（export + push）
//! - [`facade::BackupReceipt`]     — backup 完成回執（destination / filename / count）
//! - [`service::snapshot::export_to`]    — `export <path>` 子命令入口
//! - [`service::snapshot::import_from`]  — `import <path>` 子命令入口
//! - [`service::snapshot::ImportSummary`] — 匯入回執
//!
//! Layering rule: Backup is **4-layer (no dao)**.
//! Storage access 全部透過 [`crate::library::facade`]；本 context 不擁有自己的
//! `dao.rs`，亦不允許 import `rusqlite` 或 `crate::library::dao::*`
//! （`LibraryDb` 型別本身僅作為 facade 參數型別出現）。
//! Backup 對 Library 是 Conformist：順從 Library facade 既有 API，
//! 不另發明 trait / repository 抽象。

pub mod facade;
pub mod service;

// Re-export user-facing entry points so cli.rs handlers can keep using
// `backup::export_to`, `backup::import_from`, `backup::run_backup`.
// Type re-exports (`BackupReceipt`, `ImportSummary`) are part of the public PL
// even though cli.rs currently only destructures the returned values.
#[allow(unused_imports)]
pub use facade::{run_backup, BackupReceipt};
#[allow(unused_imports)]
pub use service::snapshot::{export_to, import_from, ImportSummary};

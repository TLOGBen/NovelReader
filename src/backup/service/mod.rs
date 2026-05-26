//! Backup service layer — snapshot serialization + transport.
//!
//! Layering rule: 本層不可直接依賴 SQLite client crate（rusqlite），
//! 亦不可 import 任何 `dao` module（含 `crate::library::dao`）。
//! 唯一允許的 storage access 路徑為 [`crate::library::facade`]。

pub mod snapshot;
pub mod transport;

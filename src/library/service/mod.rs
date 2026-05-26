//! Library service layer — invariants & business rules for shelf / TOC / progress.
//!
//! Layering rule (REQ-002): 本層 .rs 檔不可直接依賴 SQLite client crate，亦不可 import 任何 dao module。
//!
//! shelf.rs 為 invariant placeholder（TASK-library-03 起點；待 annotation / multi-session 落地時填入 TOC↔progress 一致性檢查 — OQ-2 trigger）。

pub mod shelf;

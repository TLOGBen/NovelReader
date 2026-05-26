//! Catalog service layer — rule DSL + HTTP scraping pipeline (pure domain).
//!
//! Layering rule: 本層不可直接依賴 SQLite client crate，亦不可 import 任何 dao module。

pub mod source;
pub mod rule;
pub mod scraper;

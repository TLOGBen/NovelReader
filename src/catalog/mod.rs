//! Catalog bounded context — 描述如何從某個小說網站抽取資料並執行抽取。
//!
//! Outbound Published Language:
//! - `SearchHit` — 搜尋結果（TASK-catalog-02 搬入）
//! - `Novel` / `Vec<ChapterMeta>` / RawContent (String) — 抓取結果（型別 owner 為 Library）
//!
//! Shared Kernel with Library: `sources` 表 + `chapters.{idx,name,url}` columns
//! (TASK-catalog-03 將從 library/dao.rs 搬出對應 method 到 catalog/dao.rs)。

pub mod service;
pub mod dao;
pub mod facade;

pub use service::source::BookSource;

use serde::{Deserialize, Serialize};

/// Published Language: Catalog 對外發佈的搜尋結果型別。Cross-context use by Presentation / Library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub source_url: String,
    pub name: String,
    pub author: Option<String>,
    pub book_url: String,
    pub kind: Option<String>,
    pub intro: Option<String>,
}

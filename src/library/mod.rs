//! Library: 維護使用者書架 / TOC / 章節快取 / 進度
//!
//! Bounded Context: Library
//!
//! Outbound PL (對外發布之型別):
//! - [`Novel`]        — 書架條目 / 書本 metadata（Shared Kernel data type，與 Catalog 共用）
//! - [`ChapterMeta`]  — TOC 內單章描述（idx / name / url）
//! - [`Chapter`]      — 含 content 的單章
//! - [`ReadProgress`] — 閱讀進度（novel_id / chapter_index / scroll_offset）
//!
//! Responsibilities:
//! - 書架（novels 表）CRUD
//! - 章節 TOC + content 快取（chapters 表的 Library 半邊；Catalog 寫 idx/name/url，Library 寫 content）
//! - 閱讀進度（progress 表）讀寫
//!
//! Layering:
//! - [`facade`] : 對外 use-case 入口（thin wrapper）
//! - [`service`]: 業務規則 / invariants（不可 import rusqlite）
//! - [`dao`]    : SQL 唯一接觸層

pub mod dao;
pub mod facade;
pub mod service;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Novel {
    pub id: Option<i64>,
    pub source_url: String,
    pub book_url: String,
    pub name: String,
    pub author: Option<String>,
    pub intro: Option<String>,
    pub cover_url: Option<String>,
    pub toc_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterMeta {
    pub index: i64,
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub meta: ChapterMeta,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadProgress {
    pub novel_id: i64,
    pub chapter_index: i64,
    pub scroll_offset: u16,
}

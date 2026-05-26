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
pub struct SearchHit {
    pub source_url: String,
    pub name: String,
    pub author: Option<String>,
    pub book_url: String,
    pub kind: Option<String>,
    pub intro: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadProgress {
    pub novel_id: i64,
    pub chapter_index: i64,
    pub scroll_offset: u16,
}

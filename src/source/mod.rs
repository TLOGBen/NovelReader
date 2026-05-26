pub mod rule;

use serde::{Deserialize, Serialize};

/// 書源 (Book Source) — 描述如何從一個小說網站抓取資料的 JSON 設定。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookSource {
    #[serde(rename = "bookSourceUrl")]
    pub book_source_url: String,
    #[serde(rename = "bookSourceName")]
    pub book_source_name: String,
    #[serde(rename = "bookSourceGroup", default)]
    pub book_source_group: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(rename = "bookUrlPattern", default)]
    pub book_url_pattern: Option<String>,
    #[serde(default)]
    pub header: Option<String>,

    #[serde(rename = "ruleSearch", default)]
    pub rule_search: SearchRule,
    #[serde(rename = "ruleBookInfo", default)]
    pub rule_book_info: BookInfoRule,
    #[serde(rename = "ruleToc", default)]
    pub rule_toc: TocRule,
    #[serde(rename = "ruleContent", default)]
    pub rule_content: ContentRule,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchRule {
    pub url: Option<String>,
    #[serde(rename = "bookList")]
    pub book_list: Option<String>,
    pub name: Option<String>,
    pub author: Option<String>,
    pub kind: Option<String>,
    pub intro: Option<String>,
    #[serde(rename = "bookUrl")]
    pub book_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BookInfoRule {
    pub name: Option<String>,
    pub author: Option<String>,
    pub kind: Option<String>,
    pub intro: Option<String>,
    #[serde(rename = "coverUrl")]
    pub cover_url: Option<String>,
    #[serde(rename = "tocUrl")]
    pub toc_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TocRule {
    #[serde(rename = "chapterList")]
    pub chapter_list: Option<String>,
    #[serde(rename = "chapterName")]
    pub chapter_name: Option<String>,
    #[serde(rename = "chapterUrl")]
    pub chapter_url: Option<String>,
    #[serde(rename = "nextTocUrl")]
    pub next_toc_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentRule {
    pub content: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "nextContentUrl")]
    pub next_content_url: Option<String>,
    #[serde(rename = "replaceRegex")]
    pub replace_regex: Option<String>,
}

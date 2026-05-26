use anyhow::{anyhow, Result};
use scraper::Html;
use wreq::Client;
use wreq_util::Emulation;

use crate::library::{ChapterMeta, Novel};
use crate::catalog::SearchHit;
use crate::catalog::service::rule;
use crate::catalog::BookSource;

pub struct Scraper {
    client: Client,
}

impl Scraper {
    pub fn new() -> Result<Self> {
        // Impersonate Chrome's TLS / JA3 / HTTP-2 fingerprint so Cloudflare-protected
        // sites (e.g., uukanshu.cc) don't reject our requests at the TLS layer.
        let client = Client::builder()
            .emulation(Emulation::Chrome131)
            .build()?;
        Ok(Self { client })
    }

    async fn fetch(&self, url: &str, headers_json: &Option<String>) -> Result<(String, String)> {
        let mut req = self.client.get(url);
        if let Some(hjson) = headers_json {
            if let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(hjson) {
                for (k, v) in map {
                    if let Some(s) = v.as_str() {
                        req = req.header(k, s);
                    }
                }
            }
        }
        let resp = req.send().await?;
        let final_url = resp.uri().to_string();
        let body = resp.text().await?;
        Ok((final_url, body))
    }

    pub async fn search(&self, src: &BookSource, keyword: &str) -> Result<Vec<SearchHit>> {
        let url_tpl = src.rule_search.url.as_deref()
            .ok_or_else(|| anyhow!("source {:?} has no search url", src.book_source_name))?;
        let url = url_tpl
            .replace("{{key}}", &urlencoding::encode(keyword))
            .replace("searchKey", &urlencoding::encode(keyword));
        let url = crate::utils::url::resolve(&src.book_source_url, &url)?;

        let (final_url, body) = self.fetch(&url, &src.header).await?;
        let doc = Html::parse_document(&body);

        let list_rule = src.rule_search.book_list.as_deref()
            .ok_or_else(|| anyhow!("missing ruleSearch.bookList"))?;
        let nodes = rule::select_nodes(&doc, list_rule)?;

        let mut hits = Vec::new();
        for n in nodes {
            let name = pick(&n, src.rule_search.name.as_deref())?;
            let author = pick(&n, src.rule_search.author.as_deref())?;
            let book_url = pick(&n, src.rule_search.book_url.as_deref())?;
            let kind = pick(&n, src.rule_search.kind.as_deref())?;
            let intro = pick(&n, src.rule_search.intro.as_deref())?;
            let Some(name) = name else { continue };
            let Some(book_url) = book_url else { continue };
            hits.push(SearchHit {
                source_url: src.book_source_url.clone(),
                name,
                author,
                book_url: crate::utils::url::resolve(&final_url, &book_url)?,
                kind,
                intro,
            });
        }
        Ok(hits)
    }

    pub async fn fetch_info(&self, src: &BookSource, book_url: &str) -> Result<Novel> {
        let (final_url, body) = self.fetch(book_url, &src.header).await?;
        let doc = Html::parse_document(&body);

        let name = rule::extract_doc(&doc, src.rule_book_info.name.as_deref().unwrap_or("h1"))?
            .unwrap_or_else(|| "Unknown".into());
        let author = pick_doc(&doc, src.rule_book_info.author.as_deref())?;
        let intro = pick_doc(&doc, src.rule_book_info.intro.as_deref())?;
        let cover = pick_doc(&doc, src.rule_book_info.cover_url.as_deref())?
            .map(|c| crate::utils::url::resolve(&final_url, &c).unwrap_or(c));
        let toc_url = pick_doc(&doc, src.rule_book_info.toc_url.as_deref())?
            .map(|t| crate::utils::url::resolve(&final_url, &t).unwrap_or(t))
            .or(Some(final_url.clone()));

        Ok(Novel {
            id: None,
            source_url: src.book_source_url.clone(),
            book_url: book_url.to_string(),
            name,
            author,
            intro,
            cover_url: cover,
            toc_url,
        })
    }

    pub async fn fetch_toc(&self, src: &BookSource, toc_url: &str) -> Result<Vec<ChapterMeta>> {
        let (final_url, body) = self.fetch(toc_url, &src.header).await?;
        let doc = Html::parse_document(&body);

        let list_rule = src.rule_toc.chapter_list.as_deref()
            .ok_or_else(|| anyhow!("missing ruleToc.chapterList"))?;
        let name_rule = src.rule_toc.chapter_name.as_deref().unwrap_or("&@text");
        let url_rule = src.rule_toc.chapter_url.as_deref().unwrap_or("&@href");

        let nodes = rule::select_nodes(&doc, list_rule)?;
        let mut chapters = Vec::new();
        for (i, n) in nodes.into_iter().enumerate() {
            let name = rule::extract_within(n, name_rule)?
                .unwrap_or_else(|| format!("Chapter {}", i + 1));
            let Some(href) = rule::extract_within(n, url_rule)? else { continue };
            let abs = crate::utils::url::resolve(&final_url, &href)?;
            chapters.push(ChapterMeta { index: i as i64, name, url: abs });
        }
        Ok(chapters)
    }

    pub async fn fetch_content(&self, src: &BookSource, chapter_url: &str) -> Result<String> {
        let (_, body) = self.fetch(chapter_url, &src.header).await?;
        let doc = Html::parse_document(&body);
        let content_rule = src.rule_content.content.as_deref()
            .ok_or_else(|| anyhow!("missing ruleContent.content"))?;
        // Content rule selects MANY nodes (paragraphs); concatenate them all.
        let parts = rule::extract_all_doc(&doc, content_rule)?;
        let raw = if parts.is_empty() {
            rule::extract_doc(&doc, content_rule)?.unwrap_or_default()
        } else {
            parts.join("\n\n")
        };
        let mut text = normalize_paragraphs(&raw);
        if let Some(re_str) = src.rule_content.replace_regex.as_deref() {
            if !re_str.is_empty() {
                if let Ok(re) = regex::Regex::new(re_str) {
                    text = re.replace_all(&text, "").into_owned();
                }
            }
        }
        Ok(text)
    }
}

fn pick(node: &scraper::ElementRef<'_>, rule_str: Option<&str>) -> Result<Option<String>> {
    match rule_str {
        Some(r) => rule::extract_within(*node, r),
        None => Ok(None),
    }
}

fn pick_doc(doc: &Html, rule_str: Option<&str>) -> Result<Option<String>> {
    match rule_str {
        Some(r) => rule::extract_doc(doc, r),
        None => Ok(None),
    }
}

/// Collapse HTML / weird whitespace into clean paragraphs.
fn normalize_paragraphs(s: &str) -> String {
    let cleaned = s
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</p>", "\n\n")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"");
    // Strip remaining tags.
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    let stripped = re.replace_all(&cleaned, "");
    stripped
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

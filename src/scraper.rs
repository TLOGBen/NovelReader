use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use scraper::Html;
use url::Url;

use crate::models::{ChapterMeta, Novel, SearchHit};
use crate::source::{rule, BookSource};

pub struct Scraper {
    client: Client,
}

impl Scraper {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) novel-looker/0.1")
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
        let final_url = resp.url().to_string();
        let body = resp.text().await?;
        Ok((final_url, body))
    }

    pub async fn search(&self, src: &BookSource, keyword: &str) -> Result<Vec<SearchHit>> {
        let url_tpl = src.rule_search.url.as_deref()
            .ok_or_else(|| anyhow!("source {:?} has no search url", src.book_source_name))?;
        let url = url_tpl
            .replace("{{key}}", &urlencoding::encode(keyword))
            .replace("searchKey", &urlencoding::encode(keyword));
        let url = resolve(&src.book_source_url, &url)?;

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
                book_url: resolve(&final_url, &book_url)?,
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
            .map(|c| resolve(&final_url, &c).unwrap_or(c));
        let toc_url = pick_doc(&doc, src.rule_book_info.toc_url.as_deref())?
            .map(|t| resolve(&final_url, &t).unwrap_or(t))
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
            let abs = resolve(&final_url, &href)?;
            chapters.push(ChapterMeta { index: i as i64, name, url: abs });
        }
        Ok(chapters)
    }

    pub async fn fetch_content(&self, src: &BookSource, chapter_url: &str) -> Result<String> {
        let (_, body) = self.fetch(chapter_url, &src.header).await?;
        let doc = Html::parse_document(&body);
        let content_rule = src.rule_content.content.as_deref()
            .ok_or_else(|| anyhow!("missing ruleContent.content"))?;
        let raw = rule::extract_doc(&doc, content_rule)?
            .unwrap_or_default();
        Ok(normalize_paragraphs(&raw))
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

fn resolve(base: &str, href: &str) -> Result<String> {
    let base = Url::parse(base).with_context(|| format!("bad base url {base}"))?;
    let resolved = base.join(href).with_context(|| format!("bad relative url {href}"))?;
    Ok(resolved.to_string())
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

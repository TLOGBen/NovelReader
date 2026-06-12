//! `epub <novel_id> <path>` — 把書架上某本書「已快取」的章節匯出成 EPUB 電子書。
//!
//! 純離線封裝：只讀 `library::facade`，**不**會為缺內容的章節上網抓取
//! （與 `read` 不同）；沒快取的章節跳過並提示使用者先 `sync`。
//! `build_epub` 是 pure helper（吃 PL 型別、回 `Result`），方便 UT 直接驗。

use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use epub_builder::{EpubBuilder, EpubContent, ReferenceType, ZipLibrary};

use crate::library::facade as library_facade;
use crate::library::{ChapterMeta, Novel};
use crate::presentation::AppContext;

pub async fn handle(novel_id: i64, path: PathBuf, ctx: &mut AppContext) -> Result<()> {
    let novel = library_facade::get_novel(&ctx.db, novel_id)?
        .ok_or_else(|| anyhow!("找不到小說 #{novel_id}"))?;
    let chapters = library_facade::list_chapters(&ctx.db, novel_id)?;
    if chapters.is_empty() {
        return Err(anyhow!("小說 #{novel_id} 沒有章節；先跑 sync"));
    }

    // 只收「已有快取內容」的章節；缺的計數後跳過（離線匯出不主動抓取）。
    let mut sections: Vec<(ChapterMeta, String)> = Vec::new();
    let mut missing = 0usize;
    for meta in &chapters {
        match library_facade::get_chapter(&ctx.db, novel_id, meta.index)? {
            Some(ch) => sections.push((meta.clone(), ch.content)),
            None => missing += 1,
        }
    }
    if sections.is_empty() {
        return Err(anyhow!(
            "小說 #{novel_id} 沒有任何已快取章節可匯出；先跑 sync 取回內文"
        ));
    }

    build_epub(&novel, &sections, &path)
        .with_context(|| format!("產生 EPUB 失敗：{}", path.display()))?;

    if missing > 0 {
        eprintln!("⚠ {missing} 章無快取內容已跳過；sync 取回後再匯出可完整");
    }
    println!("✓ 匯出 {} 章 → {}", sections.len(), path.display());
    Ok(())
}

/// Pure builder：把 Novel metadata + (章節 meta, 內文) 串成 EPUB 寫到 `path`。
/// 無 DB、無 AppContext —— 只吃 PL 型別，UT 可直接呼叫。
fn build_epub(novel: &Novel, sections: &[(ChapterMeta, String)], path: &Path) -> Result<()> {
    let mut builder = EpubBuilder::new(ZipLibrary::new()?)?;
    builder.metadata("title", novel.name.clone())?;
    if let Some(author) = novel.author.as_deref() {
        if !author.is_empty() {
            builder.metadata("author", author.to_string())?;
        }
    }
    builder.metadata("lang", "zh")?;
    builder.inline_toc();

    for (i, (meta, content)) in sections.iter().enumerate() {
        let xhtml = chapter_xhtml(&meta.name, content);
        let href = format!("chapter_{:04}.xhtml", i + 1);
        builder.add_content(
            EpubContent::new(href, xhtml.as_bytes())
                .title(meta.name.clone())
                .reftype(ReferenceType::Text),
        )?;
    }

    let file = File::create(path)
        .with_context(|| format!("建立檔案失敗：{}", path.display()))?;
    builder.generate(file)?;
    Ok(())
}

/// 把純文字章節內文包成最小合法 XHTML：每段非空行 → `<p>`，標題 → `<h1>`。
/// 內文是 scraper `normalize_paragraphs` 後的純文字（`\n` 分段），這裡做 HTML escape。
fn chapter_xhtml(title: &str, content: &str) -> String {
    let mut body = String::new();
    for line in content.split('\n') {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        body.push_str("<p>");
        body.push_str(&escape(t));
        body.push_str("</p>\n");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head>\
         <meta charset=\"utf-8\"/><title>{t}</title></head>\
         <body><h1>{t}</h1>\n{body}</body></html>",
        t = escape(title),
        body = body,
    )
}

/// 最小 XML escape：轉義 `& < >`，並丟棄 XML 1.0 不允許的控制字元
/// （只保留 tab / LF / CR）—— 畸形書源可能夾帶 NUL / form-feed 等 byte，
/// 直接寫進 XHTML 會讓整份文件 not well-formed、壞掉 EPUB。
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        // XML 1.0 §2.2 合法 char：< 0x20 僅允許 \t \n \r。
        if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' {
            continue;
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn novel(name: &str, author: Option<&str>) -> Novel {
        Novel {
            id: Some(1),
            source_url: "https://test.example/".into(),
            book_url: "https://test.example/b/1".into(),
            name: name.into(),
            author: author.map(|s| s.into()),
            intro: None,
            cover_url: None,
            toc_url: None,
        }
    }

    fn section(idx: i64, name: &str, content: &str) -> (ChapterMeta, String) {
        (
            ChapterMeta { index: idx, name: name.into(), url: format!("u{idx}") },
            content.into(),
        )
    }

    #[test]
    fn build_epub_writes_valid_zip_with_chapters() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("novel-looker-test-{}.epub", std::process::id()));
        let n = novel("測試小說", Some("作者甲"));
        let sections = vec![
            section(0, "第一章 開始", "你好世界。\n\n第二段。"),
            section(1, "第二章 旅途", "內文 <與> &符號 escape 測試。"),
        ];

        build_epub(&n, &sections, &path).expect("build_epub ok");

        let bytes = std::fs::read(&path).expect("read back epub");
        // EPUB 是 ZIP：magic 'PK\x03\x04'。
        assert_eq!(&bytes[..4], b"PK\x03\x04", "輸出應為合法 ZIP");
        assert!(bytes.len() > 200, "epub 應有實質內容");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn chapter_xhtml_escapes_and_paragraphs() {
        let x = chapter_xhtml("標題", "第一段\n\n含 <tag> & 符號");
        assert!(x.contains("<h1>標題</h1>"));
        assert!(x.contains("<p>第一段</p>"));
        assert!(x.contains("&lt;tag&gt;"));
        assert!(x.contains("&amp;"));
        // 空行不該產生空 <p>。
        assert!(!x.contains("<p></p>"));
    }

    #[test]
    fn escape_orders_ampersand_first() {
        assert_eq!(escape("a & b < c > d"), "a &amp; b &lt; c &gt; d");
    }

    #[test]
    fn escape_strips_illegal_control_chars_keeps_tab_nl_cr() {
        // NUL / SOH / VT / FF / US 非法 → 丟棄；\t \n \r 保留。
        let input = "a\u{0}b\u{1}c\u{B}\u{C}\u{1F}\td\ne\rf";
        assert_eq!(escape(input), "abc\td\ne\rf");
        // 含控制字元的內文產出的 XHTML 不應含該 byte。
        let x = chapter_xhtml("標題\u{0}", "段落\u{7}內容");
        assert!(!x.contains('\u{0}'));
        assert!(!x.contains('\u{7}'));
        assert!(x.contains("段落內容"));
    }
}

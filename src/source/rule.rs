//! Mini rule DSL for HTML extraction.
//!
//! Syntax:
//!   `<css_selector>[@<accessor>][##<regex>##<replacement>]`
//!   Multiple alternatives can be joined with `||` (first non-empty wins).
//!
//! Accessor:
//!   - `text` (default if omitted) — element's text content
//!   - `html` — element's inner HTML
//!   - `outerHtml` — element's outer HTML
//!   - any other token — element's attribute of that name (e.g. `href`, `src`)
//!
//! Examples:
//!   `.title`                      — text of .title
//!   `a@href`                      — href of <a>
//!   `.c@text##^\s+|\s+$##`        — text of .c with whitespace trimmed via regex
//!   `.title || h1`                — first non-empty of two selectors
//!
//! For *list* rules (bookList / chapterList) the accessor and regex parts
//! are ignored — the rule yields matching elements as sub-contexts.

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

#[derive(Debug, Clone)]
pub struct Rule {
    pub alternatives: Vec<RuleAlt>,
}

#[derive(Debug, Clone)]
pub struct RuleAlt {
    pub selector: String,
    pub accessor: Accessor,
    pub replace: Option<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum Accessor {
    Text,
    Html,
    OuterHtml,
    Attr(String),
}

pub fn parse_rule(raw: &str) -> Result<Rule> {
    let mut alternatives = Vec::new();
    for piece in raw.split("||") {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        alternatives.push(parse_alt(piece)?);
    }
    if alternatives.is_empty() {
        anyhow::bail!("rule is empty: {raw:?}");
    }
    Ok(Rule { alternatives })
}

fn parse_alt(raw: &str) -> Result<RuleAlt> {
    // Split off the ##regex##replacement tail (if any).
    let (head, replace) = if let Some(idx) = raw.find("##") {
        let rest = &raw[idx + 2..];
        let (re, rep) = match rest.find("##") {
            Some(j) => (rest[..j].to_string(), rest[j + 2..].to_string()),
            None => (rest.to_string(), String::new()),
        };
        (&raw[..idx], Some((re, rep)))
    } else {
        (raw, None)
    };

    // Split selector and accessor on the last `@`.
    let (selector, accessor) = match head.rfind('@') {
        Some(i) => (head[..i].trim().to_string(), parse_accessor(&head[i + 1..])),
        None => (head.trim().to_string(), Accessor::Text),
    };

    if selector.is_empty() {
        anyhow::bail!("rule has no selector: {raw:?}");
    }
    Ok(RuleAlt { selector, accessor, replace })
}

fn parse_accessor(tok: &str) -> Accessor {
    match tok.trim() {
        "" | "text" => Accessor::Text,
        "html" | "innerHtml" => Accessor::Html,
        "outerHtml" => Accessor::OuterHtml,
        other => Accessor::Attr(other.to_string()),
    }
}

/// Select multiple elements within `ctx` using the first alternative's selector.
/// Used for list rules (bookList, chapterList) — accessor/regex ignored.
pub fn select_nodes<'a>(ctx: &'a Html, rule_str: &str) -> Result<Vec<ElementRef<'a>>> {
    let rule = parse_rule(rule_str)?;
    for alt in &rule.alternatives {
        let sel = Selector::parse(&alt.selector)
            .map_err(|e| anyhow::anyhow!("bad selector {:?}: {e:?}", alt.selector))?;
        let nodes: Vec<_> = ctx.select(&sel).collect();
        if !nodes.is_empty() {
            return Ok(nodes);
        }
    }
    Ok(Vec::new())
}

/// Like `select_nodes` but operates on an already-selected element.
pub fn select_within<'a>(ctx: ElementRef<'a>, rule_str: &str) -> Result<Vec<ElementRef<'a>>> {
    let rule = parse_rule(rule_str)?;
    for alt in &rule.alternatives {
        let sel = Selector::parse(&alt.selector)
            .map_err(|e| anyhow::anyhow!("bad selector {:?}: {e:?}", alt.selector))?;
        let nodes: Vec<_> = ctx.select(&sel).collect();
        if !nodes.is_empty() {
            return Ok(nodes);
        }
    }
    Ok(Vec::new())
}

/// Extract a single string from the document.
pub fn extract_doc(doc: &Html, rule_str: &str) -> Result<Option<String>> {
    let rule = parse_rule(rule_str)?;
    for alt in &rule.alternatives {
        let sel = Selector::parse(&alt.selector)
            .map_err(|e| anyhow::anyhow!("bad selector {:?}: {e:?}", alt.selector))?;
        if let Some(node) = doc.select(&sel).next() {
            let raw = read_accessor(node, &alt.accessor);
            let out = apply_replace(raw, &alt.replace)?;
            if !out.trim().is_empty() {
                return Ok(Some(out));
            }
        }
    }
    Ok(None)
}

/// Extract a single string from within an element.
pub fn extract_within(ctx: ElementRef<'_>, rule_str: &str) -> Result<Option<String>> {
    let rule = parse_rule(rule_str)?;
    for alt in &rule.alternatives {
        let sel = Selector::parse(&alt.selector)
            .map_err(|e| anyhow::anyhow!("bad selector {:?}: {e:?}", alt.selector))?;
        // A leading match against `ctx` itself: if selector is e.g. "&", treat as self.
        let candidate = if alt.selector == "&" {
            Some(ctx)
        } else {
            ctx.select(&sel).next()
        };
        if let Some(node) = candidate {
            let raw = read_accessor(node, &alt.accessor);
            let out = apply_replace(raw, &alt.replace)?;
            if !out.trim().is_empty() {
                return Ok(Some(out));
            }
        }
    }
    Ok(None)
}

/// Extract all matches as strings.
pub fn extract_all_doc(doc: &Html, rule_str: &str) -> Result<Vec<String>> {
    let rule = parse_rule(rule_str)?;
    let mut out = Vec::new();
    for alt in &rule.alternatives {
        let sel = Selector::parse(&alt.selector)
            .map_err(|e| anyhow::anyhow!("bad selector {:?}: {e:?}", alt.selector))?;
        for node in doc.select(&sel) {
            let raw = read_accessor(node, &alt.accessor);
            let val = apply_replace(raw, &alt.replace)?;
            if !val.trim().is_empty() {
                out.push(val);
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    Ok(out)
}

fn read_accessor(node: ElementRef<'_>, acc: &Accessor) -> String {
    match acc {
        Accessor::Text => node.text().collect::<Vec<_>>().join("").trim().to_string(),
        Accessor::Html => node.inner_html(),
        Accessor::OuterHtml => node.html(),
        Accessor::Attr(name) => node.value().attr(name).unwrap_or("").to_string(),
    }
}

fn apply_replace(mut s: String, replace: &Option<(String, String)>) -> Result<String> {
    if let Some((pat, rep)) = replace {
        if !pat.is_empty() {
            let re = Regex::new(pat)?;
            s = re.replace_all(&s, rep.as_str()).into_owned();
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let r = parse_rule(".title").unwrap();
        assert_eq!(r.alternatives.len(), 1);
        assert_eq!(r.alternatives[0].selector, ".title");
        assert!(matches!(r.alternatives[0].accessor, Accessor::Text));
    }

    #[test]
    fn parse_attr_and_replace() {
        let r = parse_rule(".t@href##^/##https://x.com/").unwrap();
        let a = &r.alternatives[0];
        assert_eq!(a.selector, ".t");
        assert!(matches!(&a.accessor, Accessor::Attr(s) if s == "href"));
        assert_eq!(a.replace.as_ref().unwrap().0, "^/");
        assert_eq!(a.replace.as_ref().unwrap().1, "https://x.com/");
    }

    #[test]
    fn parse_alternatives() {
        let r = parse_rule(".a || h1@text").unwrap();
        assert_eq!(r.alternatives.len(), 2);
    }

    #[test]
    fn extract_text_with_fallback() {
        let html = Html::parse_fragment("<h1>Hello</h1>");
        let v = extract_doc(&html, ".missing || h1").unwrap();
        assert_eq!(v.as_deref(), Some("Hello"));
    }
}

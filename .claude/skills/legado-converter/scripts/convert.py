#!/usr/bin/env python3
"""Convert a Legado-format book source JSON into novel-looker format.

Input may be:
- A local file path (single object or JSON array)
- A URL to a yckceo content/id/<id>.html page (extracts the JSON from <pre>)
- "-" to read from stdin

Output: writes one JSON file per source. For arrays, files are named
<bookSourceName>.json (or <bookSourceUrl host>.json fallback).
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.parse
import urllib.request
from html import unescape
from pathlib import Path
from typing import Any

# ---------- rule string conversion ----------

UNSUPPORTED_MARKERS = ("@xpath:", "@js:", "<js>", "</js>", "@put", "@get", "@hetu:", "@json:")

# Bare XPath: starts with '//' or '/html' or contains '/@attr' pattern
XPATH_RE = re.compile(r"^\s*//|^/html|/@\w+(?:\s|$)|\[@\w+=")

# Legado runtime variables we can't resolve at convert time
LEGADO_VAR_RE = re.compile(r"\{\{(?!key\}|page\}|searchKey\})[a-zA-Z_][\w.]*\}\}")


def convert_rule(raw: str | None, warnings: list[str], where: str) -> str | None:
    """Translate one rule string to novel-looker DSL.

    Returns None if every alternative is unsupported.
    """
    if not raw:
        return raw
    alts_out: list[str] = []
    for alt in raw.split("||"):
        a = alt.strip()
        if not a:
            continue
        if any(m in a for m in UNSUPPORTED_MARKERS):
            warnings.append(f"{where}: skipping unsupported alt {a!r}")
            continue
        if XPATH_RE.search(a):
            warnings.append(f"{where}: skipping XPath {a!r}")
            continue
        if LEGADO_VAR_RE.search(a):
            warnings.append(f"{where}: skipping runtime-variable {a!r}")
            continue
        if a.startswith("@css:"):
            a = a[len("@css:") :].lstrip()
        # Bare accessor like "text" / "html" → current element accessor
        if a in ("text", "html", "outerHtml"):
            a = f"&@{a}"
        # Legado uses '@@' to chain a sub-selector, which is just descendant CSS.
        # Only replace standalone '@@' (not inside ##regex##rep tail).
        head, sep, tail = a.partition("##")
        head = head.replace("@@", " ")
        a = head + (sep + tail if sep else "")
        alts_out.append(a)
    if not alts_out:
        return None
    return " || ".join(alts_out)


# ---------- field-by-field conversion ----------

SEARCH_RULE_FIELDS = ("bookList", "name", "author", "kind", "intro", "bookUrl")
INFO_RULE_FIELDS = ("name", "author", "kind", "intro", "coverUrl", "tocUrl")
TOC_RULE_FIELDS = ("chapterList", "chapterName", "chapterUrl", "nextTocUrl")
CONTENT_RULE_FIELDS = ("content", "title", "nextContentUrl", "replaceRegex")


def convert_search_url(raw: str | None, warnings: list[str]) -> str | None:
    if not raw:
        return raw
    # Legado supports trailing ",{...config...}" for POST / extra headers — GET-only here.
    if ',{"method"' in raw or ',{ "method"' in raw or ',{"body"' in raw or ',{ "body"' in raw:
        warnings.append("searchUrl: POST/headers config not supported, taking URL prefix only")
        raw = raw.split(",", 1)[0].strip()
    raw = raw.replace("searchKey", "{{key}}")
    return raw


def convert_source(src: dict[str, Any]) -> tuple[dict[str, Any], list[str]]:
    warnings: list[str] = []

    out: dict[str, Any] = {
        "bookSourceUrl": src.get("bookSourceUrl", ""),
        "bookSourceName": src.get("bookSourceName", ""),
    }
    if src.get("bookSourceGroup"):
        out["bookSourceGroup"] = src["bookSourceGroup"]
    out["enabled"] = bool(src.get("enabled", True))
    if src.get("bookUrlPattern"):
        out["bookUrlPattern"] = src["bookUrlPattern"]
    if src.get("header"):
        out["header"] = src["header"]

    # searchUrl is top-level in Legado; ruleSearch.url in ours.
    legado_search = src.get("ruleSearch", {}) or {}
    search_url = convert_search_url(src.get("searchUrl"), warnings)

    rule_search: dict[str, Any] = {}
    if search_url:
        rule_search["url"] = search_url
    for f in SEARCH_RULE_FIELDS:
        if f in legado_search and legado_search[f]:
            v = convert_rule(legado_search[f], warnings, f"ruleSearch.{f}")
            if v is not None:
                rule_search[f] = v
    out["ruleSearch"] = rule_search

    out["ruleBookInfo"] = _convert_subrule(
        src.get("ruleBookInfo", {}), INFO_RULE_FIELDS, "ruleBookInfo", warnings
    )
    out["ruleToc"] = _convert_subrule(
        src.get("ruleToc", {}), TOC_RULE_FIELDS, "ruleToc", warnings
    )
    out["ruleContent"] = _convert_subrule(
        src.get("ruleContent", {}), CONTENT_RULE_FIELDS, "ruleContent", warnings
    )

    return out, warnings


def _convert_subrule(
    raw: dict[str, Any], fields: tuple[str, ...], where: str, warnings: list[str]
) -> dict[str, Any]:
    out: dict[str, Any] = {}
    raw = raw or {}
    for f in fields:
        if f in raw and raw[f]:
            v = convert_rule(raw[f], warnings, f"{where}.{f}")
            if v is not None:
                out[f] = v
    return out


# ---------- input loading ----------


def load_input(spec: str) -> list[dict[str, Any]]:
    """Return a list of Legado source dicts from a file, URL, or stdin."""
    if spec == "-":
        text = sys.stdin.read()
    elif spec.startswith("http://") or spec.startswith("https://"):
        text = _fetch_url_json_or_html(spec)
    else:
        text = Path(spec).read_text(encoding="utf-8")

    text = text.strip()
    data = json.loads(text)
    if isinstance(data, dict):
        return [data]
    if isinstance(data, list):
        return data
    raise SystemExit(f"unexpected JSON shape: {type(data).__name__}")


def _fetch_url_json_or_html(url: str) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 legado-converter"})
    with urllib.request.urlopen(req, timeout=20) as resp:  # noqa: S310
        body = resp.read().decode("utf-8", errors="replace")
    # If it parses as JSON, return as-is.
    try:
        json.loads(body)
        return body
    except json.JSONDecodeError:
        pass
    # Otherwise assume yckceo HTML; pull the second <pre>.
    pres = re.findall(r"<pre[^>]*>(.*?)</pre>", body, re.DOTALL)
    for p in reversed(pres):
        txt = unescape(re.sub(r"<[^>]+>", "", p)).strip()
        if txt.startswith("{") or txt.startswith("["):
            return txt
    raise SystemExit(f"no JSON block found in {url}")


# ---------- file naming ----------


def safe_filename(name: str, fallback: str = "source") -> str:
    name = name.strip() or fallback
    name = re.sub(r"[^\w\-.一-鿿]+", "_", name)
    return name[:80] or fallback


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("input", help='file path, URL, or "-" for stdin')
    ap.add_argument("--out-dir", default="book-sources", help="output directory")
    ap.add_argument("--stdout", action="store_true", help="print to stdout (single source only)")
    args = ap.parse_args()

    sources = load_input(args.input)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    if args.stdout and len(sources) != 1:
        print("--stdout requires exactly one source in input", file=sys.stderr)
        return 2

    total_warnings = 0
    for src in sources:
        converted, warnings = convert_source(src)
        name = converted.get("bookSourceName") or urllib.parse.urlparse(
            converted.get("bookSourceUrl", "")
        ).hostname or "source"
        if args.stdout:
            json.dump(converted, sys.stdout, ensure_ascii=False, indent=2)
            sys.stdout.write("\n")
        else:
            path = out_dir / f"{safe_filename(name)}.json"
            path.write_text(
                json.dumps(converted, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
            )
            print(f"✓ wrote {path}", file=sys.stderr)
        for w in warnings:
            print(f"  ! {name}: {w}", file=sys.stderr)
        total_warnings += len(warnings)

    print(
        f"\nConverted {len(sources)} source(s), {total_warnings} warning(s).",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

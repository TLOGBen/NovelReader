use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::backup;
use crate::config::Config;
use crate::reader;
use crate::scraper::Scraper;
use crate::source::BookSource;
use crate::storage::Storage;

#[derive(Parser, Debug)]
#[command(name = "novel-looker", version, about = "看小說 CLI (資料驅動書源)")]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// 書源管理
    Source {
        #[command(subcommand)]
        action: SourceCmd,
    },
    /// 搜尋小說
    Search {
        keyword: String,
        /// 指定書源 URL（不指定則搜全部已啟用書源）
        #[arg(long)]
        source: Option<String>,
    },
    /// 加入書架（從詳情頁 URL）
    Add {
        /// 書源 URL
        #[arg(long)]
        source: String,
        /// 小說詳情頁 URL
        book_url: String,
    },
    /// 列出書架
    Shelf,
    /// 同步章節列表
    Sync {
        novel_id: i64,
    },
    /// 抓取單一章節（純文字輸出到 stdout）
    Read {
        novel_id: i64,
        chapter_index: i64,
    },
    /// 進入 TUI 閱讀器
    Tui {
        novel_id: i64,
    },
    /// 設定管理 (~/.config/novel-looker/config.toml)
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// 匯出書架 + 進度為 JSON（不含章節內文，可重新 sync 取回）
    Export {
        /// 輸出檔案路徑
        path: PathBuf,
    },
    /// 從 JSON 匯入書架 + 進度（不影響已存在書源）
    Import {
        /// 來源檔案路徑
        path: PathBuf,
    },
    /// 依據設定執行備份（export → 推到 local / webdav backend）
    Backup,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// 顯示目前設定
    Show,
    /// 設定一個 key（如 backup.local.path / backup.backend / backup.keep）
    Set {
        key: String,
        value: String,
    },
    /// 顯示設定檔案路徑
    Path,
}

#[derive(Subcommand, Debug)]
pub enum SourceCmd {
    /// 從 JSON 檔案匯入書源
    Import { path: PathBuf },
    /// 列出已安裝的書源
    List,
}

pub async fn run(cli: Cli) -> Result<()> {
    let mut store = Storage::open()?;
    match cli.cmd {
        Cmd::Source { action } => match action {
            SourceCmd::Import { path } => {
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                // 支援單一書源或書源陣列
                let count = if text.trim_start().starts_with('[') {
                    let list: Vec<BookSource> = serde_json::from_str(&text)?;
                    for s in &list {
                        store.save_source(s)?;
                    }
                    list.len()
                } else {
                    let s: BookSource = serde_json::from_str(&text)?;
                    store.save_source(&s)?;
                    1
                };
                println!("已匯入 {count} 個書源");
            }
            SourceCmd::List => {
                let list = store.list_sources()?;
                if list.is_empty() {
                    println!("（沒有書源，先用 `source import <file>` 匯入）");
                }
                for s in list {
                    let group = s.book_source_group.as_deref().unwrap_or("-");
                    let status = if s.enabled { "✓" } else { "✗" };
                    println!("{status} [{group}] {} — {}", s.book_source_name, s.book_source_url);
                }
            }
        },
        Cmd::Search { keyword, source } => {
            let scraper = Scraper::new()?;
            let sources: Vec<BookSource> = match source {
                Some(url) => vec![store.get_source(&url)?.ok_or_else(|| anyhow!("找不到書源: {url}"))?],
                None => store.list_sources()?.into_iter().filter(|s| s.enabled).collect(),
            };
            if sources.is_empty() {
                println!("沒有可用書源");
                return Ok(());
            }
            for src in &sources {
                println!("== {} ==", src.book_source_name);
                match scraper.search(src, &keyword).await {
                    Ok(hits) => {
                        if hits.is_empty() {
                            println!("  (no results)");
                        }
                        for h in hits {
                            println!(
                                "  {} / {} -> {}",
                                h.name,
                                h.author.as_deref().unwrap_or("-"),
                                h.book_url
                            );
                        }
                    }
                    Err(e) => println!("  error: {e:#}"),
                }
            }
        }
        Cmd::Add { source, book_url } => {
            let scraper = Scraper::new()?;
            let src = store.get_source(&source)?.ok_or_else(|| anyhow!("找不到書源: {source}"))?;
            let novel = scraper.fetch_info(&src, &book_url).await?;
            let id = store.upsert_novel(&novel)?;
            println!("✓ 加入書架 (#{id}) {} / {}", novel.name, novel.author.as_deref().unwrap_or("-"));
        }
        Cmd::Shelf => {
            let novels = store.list_novels()?;
            if novels.is_empty() {
                println!("（書架空空，用 `add --source ... <book_url>` 加書）");
            }
            for n in novels {
                println!(
                    "#{} {} / {} [{}]",
                    n.id.unwrap_or(0),
                    n.name,
                    n.author.as_deref().unwrap_or("-"),
                    n.source_url
                );
            }
        }
        Cmd::Sync { novel_id } => {
            let scraper = Scraper::new()?;
            let novel = store.get_novel(novel_id)?
                .ok_or_else(|| anyhow!("找不到小說 #{novel_id}"))?;
            let src = store.get_source(&novel.source_url)?
                .ok_or_else(|| anyhow!("找不到書源 {}", novel.source_url))?;
            let toc_url = novel.toc_url.as_deref().unwrap_or(&novel.book_url);
            let chapters = scraper.fetch_toc(&src, toc_url).await?;
            let n = chapters.len();
            store.replace_toc(novel_id, &chapters)?;
            println!("✓ 同步 {n} 章");
        }
        Cmd::Read { novel_id, chapter_index } => {
            let novel = store.get_novel(novel_id)?
                .ok_or_else(|| anyhow!("找不到小說 #{novel_id}"))?;
            let chapters = store.list_chapters(novel_id)?;
            let meta = chapters.iter().find(|c| c.index == chapter_index)
                .ok_or_else(|| anyhow!("找不到第 {chapter_index} 章；先跑 sync"))?;
            // Try cache first.
            if let Some(ch) = store.get_chapter(novel_id, chapter_index)? {
                println!("# {}\n\n{}", ch.meta.name, ch.content);
                return Ok(());
            }
            let scraper = Scraper::new()?;
            let src = store.get_source(&novel.source_url)?
                .ok_or_else(|| anyhow!("找不到書源"))?;
            let content = scraper.fetch_content(&src, &meta.url).await?;
            store.save_chapter_content(novel_id, chapter_index, &content)?;
            println!("# {}\n\n{}", meta.name, content);
        }
        Cmd::Tui { novel_id } => {
            reader::run(&mut store, novel_id).await?;
        }
        Cmd::Config { action } => match action {
            ConfigCmd::Show => {
                let cfg = Config::load()?;
                let text = toml::to_string_pretty(&cfg)?;
                println!("# {}", crate::config::config_path()?.display());
                println!("{text}");
            }
            ConfigCmd::Set { key, value } => {
                let mut cfg = Config::load()?;
                let prev = cfg.set(&key, &value)?;
                cfg.save()?;
                match prev {
                    Some(p) => println!("✓ {key} = {value} (was: {p})"),
                    None => println!("✓ {key} = {value}"),
                }
            }
            ConfigCmd::Path => {
                println!("{}", crate::config::config_path()?.display());
            }
        },
        Cmd::Export { path } => {
            let n = backup::export_to(&store, &path)?;
            println!("✓ 匯出 {n} 本書 → {}", path.display());
        }
        Cmd::Import { path } => {
            let s = backup::import_from(&store, &path)?;
            println!(
                "✓ 匯入 {} 本書（{} 含進度） ← {}",
                s.added, s.with_progress, path.display()
            );
        }
        Cmd::Backup => {
            let cfg = Config::load()?;
            let receipt = backup::run_backup(&store, &cfg).await?;
            println!(
                "✓ 備份 {} 本書 [{}] → {}",
                receipt.novels, cfg.backup.backend, receipt.destination
            );
        }
    }
    Ok(())
}

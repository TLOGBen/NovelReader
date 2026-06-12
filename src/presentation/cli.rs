use clap::{Parser, Subcommand};
use std::path::PathBuf;

use anyhow::Result;

use crate::presentation::handlers;
use crate::presentation::AppContext;

#[derive(Parser, Debug)]
#[command(name = "novel-looker", version, about = "看小說 CLI (資料驅動書源)")]
pub struct Cli {
    /// 無子命令時為 `None`，由 `cli::run` 轉入 TUI 主菜單 (REQ-001 Scenario 1)。
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
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
    /// 從書架移除（hard delete：novel + chapters + progress 一併刪）
    Remove {
        /// 書架上的 novel_id
        novel_id: i64,
        /// 跳過互動式 [y/N] 確認（用於 script / batch）
        #[arg(long)]
        yes: bool,
    },
    /// 換源（將書架上某本書換綁到另一個書源；REQ-005 Scenario 6）
    SwitchSource {
        /// 書架上的 novel_id
        novel_id: i64,
        /// 該書在新源的詳情頁 URL
        new_book_url: String,
        /// 新書源 URL
        #[arg(long)]
        source: String,
    },
    /// 匯出已快取章節為 EPUB 電子書（缺內容的章節跳過，先 sync 取回）
    Epub {
        /// 書架上的 novel_id
        novel_id: i64,
        /// 輸出 .epub 檔案路徑
        path: PathBuf,
    },
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

pub async fn run(cli: Cli, mut ctx: AppContext) -> Result<()> {
    match cli.cmd {
        Some(Cmd::Source { action }) => handlers::source::handle(action, &mut ctx).await,
        Some(Cmd::Search { keyword, source }) => {
            handlers::search::handle(keyword, source, &mut ctx).await
        }
        Some(Cmd::Add { source, book_url }) => {
            handlers::add::handle(source, book_url, &mut ctx).await
        }
        Some(Cmd::Shelf) => handlers::shelf::handle(&mut ctx).await,
        Some(Cmd::Sync { novel_id }) => handlers::sync::handle(novel_id, &mut ctx).await,
        Some(Cmd::Read { novel_id, chapter_index }) => {
            handlers::read::handle(novel_id, chapter_index, &mut ctx).await
        }
        // TUI direct entry：tui handler 需要 owned AppContext（App 持有 ctx）。
        Some(Cmd::Tui { novel_id }) => handlers::tui::handle(novel_id, ctx).await,
        Some(Cmd::Config { action }) => handlers::config::handle(action, &mut ctx).await,
        Some(Cmd::Export { path }) => handlers::export::handle(path, &mut ctx).await,
        Some(Cmd::Import { path }) => handlers::import::handle(path, &mut ctx).await,
        Some(Cmd::Backup) => handlers::backup::handle(&mut ctx).await,
        Some(Cmd::Remove { novel_id, yes }) => {
            handlers::remove::handle(novel_id, yes, &mut ctx).await
        }
        Some(Cmd::SwitchSource { novel_id, new_book_url, source }) => {
            handlers::switch_source::handle(novel_id, new_book_url, source, &mut ctx).await
        }
        Some(Cmd::Epub { novel_id, path }) => handlers::epub::handle(novel_id, path, &mut ctx).await,
        // 無子命令：移交 owned ctx 給 menu handler（TUI 主菜單需 owned AppContext）。
        None => handlers::menu::handle(ctx).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// REQ-001 Scenario 1: invoking `novel-looker` with no subcommand
    /// yields `cmd: None` so the entry-point can dispatch to the TUI menu.
    #[test]
    fn cli_no_subcommand_parses_to_none() {
        let cli = Cli::try_parse_from(["novel-looker"]).expect("clap should accept no subcommand");
        assert!(cli.cmd.is_none(), "expected cmd to be None when no subcommand provided");
    }

    /// REQ-005 Scenario 6: CLI `switch-source <novel_id> <new_book_url> --source <url>`
    /// parses into the new variant carrying all three fields.
    #[test]
    fn cli_switch_source_parses_with_three_fields() {
        let cli = Cli::try_parse_from([
            "novel-looker",
            "switch-source",
            "1",
            "https://czbooks.net/n/abc",
            "--source",
            "https://czbooks.net",
        ])
        .expect("clap should accept switch-source");
        match cli.cmd {
            Some(Cmd::SwitchSource { novel_id, new_book_url, source }) => {
                assert_eq!(novel_id, 1);
                assert_eq!(new_book_url, "https://czbooks.net/n/abc");
                assert_eq!(source, "https://czbooks.net");
            }
            other => panic!("expected Cmd::SwitchSource, got {other:?}"),
        }
    }

    /// Shelf delete CLI parity (/think 2026-05-27): `remove <id> [--yes]`
    /// parses into `Cmd::Remove { novel_id, yes }`.
    #[test]
    fn cli_remove_with_yes_flag_parses() {
        let cli = Cli::try_parse_from(["novel-looker", "remove", "42", "--yes"])
            .expect("clap should accept remove --yes");
        match cli.cmd {
            Some(Cmd::Remove { novel_id, yes }) => {
                assert_eq!(novel_id, 42);
                assert!(yes);
            }
            other => panic!("expected Cmd::Remove, got {other:?}"),
        }
    }

    #[test]
    fn cli_epub_parses_novel_id_and_path() {
        let cli = Cli::try_parse_from(["novel-looker", "epub", "3", "/tmp/book.epub"])
            .expect("clap should accept epub");
        match cli.cmd {
            Some(Cmd::Epub { novel_id, path }) => {
                assert_eq!(novel_id, 3);
                assert_eq!(path, PathBuf::from("/tmp/book.epub"));
            }
            other => panic!("expected Cmd::Epub, got {other:?}"),
        }
    }

    #[test]
    fn cli_remove_without_yes_defaults_false() {
        let cli = Cli::try_parse_from(["novel-looker", "remove", "7"])
            .expect("clap should accept remove without --yes");
        match cli.cmd {
            Some(Cmd::Remove { novel_id, yes }) => {
                assert_eq!(novel_id, 7);
                assert!(!yes);
            }
            other => panic!("expected Cmd::Remove, got {other:?}"),
        }
    }

    /// REQ-001 Scenario 4: existing subcommands still parse normally
    /// (regression guard against the Option<Cmd> migration).
    #[test]
    fn cli_existing_shelf_subcommand_still_parses() {
        let cli =
            Cli::try_parse_from(["novel-looker", "shelf"]).expect("clap should accept shelf");
        assert!(matches!(cli.cmd, Some(Cmd::Shelf)));
    }
}

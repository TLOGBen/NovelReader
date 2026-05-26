use clap::{Parser, Subcommand};
use std::path::PathBuf;

use anyhow::Result;

use crate::presentation::handlers;
use crate::presentation::AppContext;

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

pub async fn run(cli: Cli, ctx: &mut AppContext) -> Result<()> {
    match cli.cmd {
        Cmd::Source { action } => handlers::source::handle(action, ctx).await,
        Cmd::Search { keyword, source } => handlers::search::handle(keyword, source, ctx).await,
        Cmd::Add { source, book_url } => handlers::add::handle(source, book_url, ctx).await,
        Cmd::Shelf => handlers::shelf::handle(ctx).await,
        Cmd::Sync { novel_id } => handlers::sync::handle(novel_id, ctx).await,
        Cmd::Read { novel_id, chapter_index } => handlers::read::handle(novel_id, chapter_index, ctx).await,
        Cmd::Tui { novel_id } => handlers::tui::handle(novel_id, ctx).await,
        Cmd::Config { action } => handlers::config::handle(action, ctx).await,
        Cmd::Export { path } => handlers::export::handle(path, ctx).await,
        Cmd::Import { path } => handlers::import::handle(path, ctx).await,
        Cmd::Backup => handlers::backup::handle(ctx).await,
    }
}

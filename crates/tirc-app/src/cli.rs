use std::path::PathBuf;

use clap::Parser;

/// TIRC command-line interface.
#[derive(Debug, Clone, Parser)]
#[command(name = "tirc", about = "Terminal IRC client")]
pub struct Cli {
    /// Override config file path.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Profile name to auto-connect on startup.
    #[arg(long)]
    pub profile: Option<String>,
    /// Disable session restore.
    #[arg(long)]
    pub no_restore: bool,
    /// Enable debug logging.
    #[arg(long)]
    pub debug: bool,
}

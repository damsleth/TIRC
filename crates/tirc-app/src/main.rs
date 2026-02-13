mod app;
mod cli;

use clap::Parser;
use cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    app::setup_tracing(cli.debug);

    if let Err(err) = app::run(cli).await {
        eprintln!("tirc error: {err:#}");
        std::process::exit(1);
    }
}

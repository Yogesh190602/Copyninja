mod config;
mod content;
mod daemon;
mod picker;
mod storage;
mod sync;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "copyninja", about = "Clipboard history manager for Linux", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the clipboard monitoring daemon
    Daemon,
    /// Open the clipboard picker UI
    Pick,
}

fn main() {
    let config = config::load();

    // Initialize logging from config (RUST_LOG env var overrides)
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", &config.log_level);
    }
    env_logger::init();

    // Initialize global storage
    storage::init(&config);

    let cli = Cli::parse();
    match cli.command {
        Commands::Daemon => daemon::run(&config),
        Commands::Pick => picker::run(&config),
    }
}

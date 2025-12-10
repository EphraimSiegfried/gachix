use clap::{Parser, Subcommand};
use std::path::PathBuf;
mod git_store;
mod http_server;
mod nar;
mod nix_interface;

use crate::http_server::start_server;
use crate::nix_interface::path::NixPath;
use anyhow::Result;
use git_store::store::Store;
use tokio::runtime::Runtime;
use tracing::Level;
use tracing_subscriber::fmt;
mod settings;

fn main() -> Result<()> {
    let args = Args::parse();

    let settings = settings::load_config(&args.config.unwrap_or("".to_string()))?;

    fmt::Subscriber::builder()
        .with_max_level(Level::DEBUG)
        .init();

    let args = Args::parse();
    let cache = Store::new(settings.store)?;

    match args.cmd {
        Command::Add(x) => x.run(&cache)?,
        Command::List(x) => x.run(&cache)?,
        Command::Serve(x) => x.run(cache, settings.server)?,
    };
    Ok(())
}

#[derive(Parser)]
struct Args {
    #[clap(short, long)]
    config: Option<String>,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    Add(Add),
    List(List),
    Serve(Serve),
}

#[derive(Parser)]
struct Add {
    file_path: PathBuf,
    #[arg(short, long, action)]
    single: bool,
}
impl Add {
    async fn run_async(&self, cache: &Store) -> Result<()> {
        let path = NixPath::new(&self.file_path)?;
        cache.peer_health_check().await;
        if self.single {
            cache.add_single(&path).await?;
        } else {
            cache.add_closure(&path).await?;
        }
        Ok(())
    }

    fn run(&self, cache: &Store) -> Result<()> {
        let rt = Runtime::new()?;
        rt.block_on(self.run_async(cache))
    }
}

#[derive(Parser)]
struct List {}
impl List {
    fn run(&self, cache: &Store) -> Result<()> {
        let result = cache.list_entries()?;
        result.iter().for_each(|e| println!("{e}"));
        Ok(())
    }
}

#[derive(Parser)]
struct Serve {}
impl Serve {
    fn run(&self, cache: Store, server_settings: settings::Server) -> Result<()> {
        start_server(&server_settings.host, server_settings.port, cache)?;
        Ok(())
    }
}

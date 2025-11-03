use clap::{Parser, Subcommand};
use std::path::PathBuf;
mod git_store;
mod http_server;
mod nar;
mod nix_interface;
use crate::http_server::start_server;
use crate::nix_interface::path::NixPath;
use anyhow::Result;
use git_store::{GitRepo, store::Store};
use tokio::runtime::Runtime;
use tracing::Level;
use tracing_subscriber::fmt;

fn main() -> Result<()> {
    fmt::Subscriber::builder()
        .with_max_level(Level::DEBUG)
        .init();

    let args = Args::parse();
    let repo = GitRepo::new(&args.store_path)?;
    let cache = Store::new(repo)?;

    match args.cmd {
        Command::Add(x) => x.run(&cache)?,
        Command::List(x) => x.run(&cache)?,
        Command::Serve(x) => x.run(cache)?,
    };
    Ok(())
}

#[derive(Parser)]
struct Args {
    #[clap(short, long, default_value("cache"))]
    store_path: PathBuf,
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
}

impl Add {
    async fn run_async(&self, cache: &Store) -> Result<()> {
        let path = NixPath::new(&self.file_path)?;
        cache.add_closure(&path).await?;
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
struct Serve {
    #[clap(long, default_value("8080"))]
    port: u16,
    #[clap(long, default_value("localhost"))]
    host: String,
}
impl Serve {
    fn run(&self, cache: Store) -> Result<()> {
        start_server(&self.host, self.port, cache)?;
        Ok(())
    }
}

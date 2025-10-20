use clap::{Parser, Subcommand};
use std::{io::Write, path::PathBuf};
mod git_store;
mod nar;
use std::io::{self};
mod nix_cache_server;
use crate::nix_cache_server::start_server;
use anyhow::Result;
use git_store::{GitStore, nar_info, store_entry};
use tracing::Level;
use tracing_subscriber::fmt;

fn main() -> Result<()> {
    fmt::Subscriber::builder()
        .with_max_level(Level::DEBUG)
        .init();

    let args = Args::parse();
    let cache = GitStore::new(&args.store_path)?;

    match args.cmd {
        Command::Add(x) => x.run(&cache)?,
        Command::Get(x) => x.run(&cache)?,
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
    Get(Get),
    List(List),
    Serve(Serve),
}

#[derive(Parser)]
struct Add {
    file_path: PathBuf,
}

impl Add {
    fn run(&self, cache: &GitStore) -> Result<()> {
        store_entry::add_entry(&cache, &self.file_path)?;
        Ok(())
    }
}

#[derive(Parser)]
struct Get {
    hash_id: String,
}

impl Get {
    fn run(&self, cache: &GitStore) -> Result<()> {
        let result = store_entry::get_as_nar(&cache, &self.hash_id)?;
        match result {
            Some(result) => io::stdout()
                .write_all(&result)
                .expect("Failed to write result to stdout"),
            _ => println!("Entry not in cache"),
        };
        Ok(())
    }
}

#[derive(Parser)]
struct List {}

impl List {
    fn run(&self, cache: &GitStore) -> Result<()> {
        let result = nar_info::list(cache)?;
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
    fn run(&self, cache: GitStore) -> Result<()> {
        start_server(&self.host, self.port, cache)?;
        Ok(())
    }
}

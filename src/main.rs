use clap::{Parser, Subcommand};
use std::path::PathBuf;
mod store;
use crate::store::CaCache;

fn main() -> Result<(), git2::Error> {
    let args = Args::parse();
    let cache = CaCache::new(&args.store_path)?;

    match args.cmd {
        Command::Add(x) => x.run(&cache)?,
        Command::Get(x) => x.run(&cache),
        Command::List(x) => x.run(&cache),
    };
    Ok(())
}

#[derive(Parser)]
struct Args {
    #[clap(long, default_value("cache"))]
    store_path: PathBuf,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    Add(Add),
    Get(Get),
    List(List),
}

#[derive(Parser)]
struct Add {
    filepath: PathBuf,
}

impl Add {
    fn run(&self, cache: &CaCache) -> Result<(), git2::Error> {
        let (hash, blob_id) = cache.add(&self.filepath)?;
        println!("Key: {}, Value: {}", hash, blob_id);
        Ok(())
    }
}

#[derive(Parser)]
struct Get {
    hash_id: String,
}

impl Get {
    fn run(&self, cache: &CaCache) {
        let result = cache.query(&self.hash_id);
        match result {
            Some(result) => println!("{}", result),
            None => println!("No corresponding value found!"),
        }
    }
}

#[derive(Parser)]
struct List {}

impl List {
    fn run(&self, cache: &CaCache) {
        let result = cache.list_entries();
        match result {
            Some(result) => result.iter().for_each(|e| println!("{e}")),
            None => println!("No entries"),
        }
    }
}

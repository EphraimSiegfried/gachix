use clap::{Parser, Subcommand};
mod store;
use crate::store::CaCache;

fn main() -> Result<(), git2::Error> {
    let args = Args::parse();
    let cache = CaCache::new(&args.store_path)?;

    match args.cmd {
        Command::Add(x) => x.run(&cache)?,
        Command::Get(x) => x.run(&cache),
    };
    Ok(())
}

#[derive(Parser)]
struct Args {
    #[clap(long, default_value("cache"))]
    store_path: String,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    Add(Add),
    Get(Get),
}

#[derive(Parser)]
struct Add {
    filepath: String,
}

impl Add {
    fn run(&self, cache: &CaCache) -> Result<(), git2::Error> {
        let (hash, blob_id) = cache.add(&self.filepath)?;
        println!("Hash: {}, Blob ID: {}", hash, blob_id);
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
            None => println!("No corresponding Blob found!"),
        }
    }
}

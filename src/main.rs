use clap::{Parser, Subcommand};
use std::{io::Write, path::PathBuf};
mod nar;
mod store;
use crate::store::CaCache;
use std::io;
mod server;
use crate::server::start_server;
use tracing_subscriber;

fn main() -> Result<(), git2::Error> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let cache = CaCache::new(&args.store_path)?;

    match args.cmd {
        Command::Add(x) => x.run(&cache)?,
        Command::Get(x) => x.run(&cache),
        Command::List(x) => x.run(&cache),
        Command::Serve(x) => x.run(cache),
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
    Serve(Serve),
}

#[derive(Parser)]
struct Add {
    filepath: PathBuf,
}

impl Add {
    fn run(&self, cache: &CaCache) -> Result<(), git2::Error> {
        let (hash, blob_id) = cache.add(&self.filepath)?;
        println!("Key: {}, Value: {}", hash, blob_id.to_string());
        Ok(())
    }
}

#[derive(Parser)]
struct Get {
    hash_id: String,
}

impl Get {
    fn run(&self, cache: &CaCache) {
        let result = cache.get_nar(&self.hash_id).unwrap();
        io::stdout()
            .write_all(&result)
            .expect("Failed to write result to stdout");
    }
}

#[derive(Parser)]
struct List {}

impl List {
    fn run(&self, cache: &CaCache) {
        let result = cache.list_keys();
        match result {
            Some(result) => result.iter().for_each(|e| println!("{e}")),
            None => println!("No entries"),
        }
    }
}

#[derive(Parser)]
struct Serve {
    #[clap(default_value("8080"))]
    port: u16,
    #[clap(default_value("localhost"))]
    host: String,
}
impl Serve {
    fn run(&self, cache: CaCache) {
        start_server(&self.host, self.port).unwrap()
    }
}

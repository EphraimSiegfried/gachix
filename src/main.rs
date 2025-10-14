use clap::{Parser, Subcommand};
use liblzma::bufread::XzDecoder;
use std::process;
use std::{io::Write, path::PathBuf};
mod git_store;
mod nar;
use std::io::{self, BufReader, Cursor, Read};
mod nix_cache_server;
use crate::nix_cache_server::start_server;
use anyhow::Result;
use git_store::GitStore;
use std::fs::{self, File};
use tracing::{Level, trace};
use tracing_subscriber::fmt;

const NARINFO_REF: &str = "refs/NARINFO";
const SUPER_REF: &str = "refs/SUPER";

fn main() -> Result<()> {
    fmt::Subscriber::builder()
        .with_max_level(Level::TRACE)
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
    narinfo_path: PathBuf,
    xz_path: PathBuf,
}

impl Add {
    fn run(&self, cache: &GitStore) -> Result<()> {
        let xz_file = File::open(&self.xz_path).unwrap_or_else(|err| {
            println!("Could not read nar file: {err}");
            process::exit(1);
        });
        let xz_reader = BufReader::new(xz_file);

        let narinfo_content = fs::read(&self.narinfo_path).unwrap_or_else(|err| {
            println!("Could not read narinfo file: {err}");
            process::exit(1);
        });

        let mut contents = Vec::new();
        let mut decompressor = XzDecoder::new(xz_reader);
        decompressor.read_to_end(&mut contents)?;

        trace!("Adding Narinfo");
        cache.add_file(
            &self.narinfo_path.file_stem().unwrap().to_str().unwrap(),
            &narinfo_content,
            NARINFO_REF,
        )?;
        trace!("Adding Nar");
        cache.add_nar(
            &self.xz_path.file_stem().unwrap().to_str().unwrap(),
            Cursor::new(contents),
            SUPER_REF,
        )?;
        Ok(())
    }
}

#[derive(Parser)]
struct Get {
    hash_id: String,
}

impl Get {
    fn run(&self, cache: &GitStore) -> Result<()> {
        let result = cache.get_nar(&self.hash_id, SUPER_REF)?;
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
        let result = cache.list_keys(SUPER_REF)?;
        result.iter().for_each(|e| println!("{e}"));
        Ok(())
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
    fn run(&self, cache: GitStore) -> Result<()> {
        start_server(&self.host, self.port, cache)?;
        Ok(())
    }
}

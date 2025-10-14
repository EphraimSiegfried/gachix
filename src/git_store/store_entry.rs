use std::io::{BufReader, Cursor, Read};
use std::path::Path;
use std::{fs::File, path::PathBuf};

use crate::git_store::GitStore;

use anyhow::Result;
use liblzma::bufread::{XzDecoder, XzEncoder};

const SUPER_REF: &str = "refs/SUPER";

pub fn get_as_xz(cache: &GitStore, key: &str) -> Result<Option<Vec<u8>>> {
    let mut nar = Vec::new(); // TODO: Implement as stream
    let res = cache.get_tree_as_nar(&mut nar, key, SUPER_REF)?;
    if res.is_none() {
        return Ok(None);
    }
    let mut compressed = Vec::new();
    let mut compressor = XzEncoder::new(Cursor::new(nar), 6);
    std::io::copy(&mut compressor, &mut compressed)?;
    Ok(Some(compressed))
}

pub fn add_xz_file(cache: &GitStore, path: &PathBuf) -> Result<()> {
    let xz_file = File::open(&path)?;
    let xz_reader = BufReader::new(xz_file);

    let mut contents = Vec::new();
    let mut decompressor = XzDecoder::new(xz_reader);
    decompressor.read_to_end(&mut contents)?;

    let key = path.file_stem().unwrap().to_str().unwrap();
    let key = Path::new(key).file_stem().unwrap().to_str().unwrap();

    cache.add_nar(key, Cursor::new(contents), SUPER_REF)?;
    Ok(())
}

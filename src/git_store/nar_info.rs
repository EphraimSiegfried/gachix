use std::{fs, path::PathBuf};

use crate::git_store::GitStore;
use anyhow::Result;

const NARINFO_REF: &str = "refs/NARINFO";

// struct NarInfo {
//     store_path: String,
//     url: String,
//     comporession_type: String,
//     file_hash: String,
//     file_size: u64,
//     nar_hash: String,
//     nar_size: u64,
//     deriver: String,
//     system: String,
//     references: Vec<String>,
// }

pub fn get_from_tree(cache: &GitStore, key: &str) -> Result<Option<Vec<u8>>> {
    cache.get_blob(key, NARINFO_REF)
}

pub fn add_file(cache: &GitStore, path: &PathBuf, key: &str) -> Result<()> {
    let content = fs::read(path)?;
    cache.add_file_content(key, &content, NARINFO_REF)?;
    Ok(())
}

pub fn exists(cache: &GitStore, key: &str) -> bool {
    cache.query(key, NARINFO_REF).is_some()
}
pub fn list(cache: &GitStore) -> Result<Vec<String>> {
    cache.list_keys(NARINFO_REF)
}

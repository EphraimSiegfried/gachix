use std::{fmt::Display, fs, path::PathBuf};

use crate::git_store::GitStore;
use anyhow::Result;
use nix_base32::to_nix_base32;

const NARINFO_REF: &str = "refs/NARINFO";

pub struct NarInfo {
    store_path: String,
    url: String,
    compression_type: String,
    file_hash: Vec<u8>,
    file_size: u64,
    nar_hash: Vec<u8>,
    nar_size: u64,
    deriver: String,
    system: String,
    references: Vec<String>,
}

impl NarInfo {
    pub fn new(store_path: String, url: String) -> Self {
        Self {
            store_path: store_path,
            url: url,
            compression_type: "".to_string(),
            file_hash: Vec::new(),
            file_size: 0,
            nar_hash: Vec::new(),
            nar_size: 0,
            deriver: "".to_string(),
            system: "".to_string(),
            references: Vec::new(),
        }
    }
}

impl Display for NarInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys = [
            "StorePath",
            "URL",
            "Compression",
            "FileHash",
            "NarHash",
            "NarSize",
            "Deriver",
            "System",
            "References",
            "Sig",
        ];
        let values = [
            &self.store_path,
            &self.url,
            &self.compression_type,
            "",
            // &to_nix_base32(&self.file_hash),
            &self.file_size.to_string(),
            "",
            // &to_nix_base32(&self.nar_hash),
            &self.nar_size.to_string(),
            &self.deriver,
            &self.system,
            &self.references.join(" "),
        ];
        for (key, value) in keys.iter().zip(values) {
            write!(f, "{}: {}\n", key, value)?;
        }
        Ok(())
    }
}

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

// fn sha256_hash(buf: &[u8]) -> Vec<u8> {
//     let mut hasher = Sha256::new();
//     hasher.update(buf);
//     hasher.finalize().to_vec()
// }

fn base32_encode(hash: &[u8]) -> String {
    to_nix_base32(hash)
}

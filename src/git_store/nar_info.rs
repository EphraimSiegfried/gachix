use std::{fmt::Display, fs, path::PathBuf};

use crate::git_store::GitRepo;
use anyhow::Result;
use nix_base32::to_nix_base32;

const NARINFO_REF: &str = "refs/NARINFO";

pub struct NarInfo<'a> {
    store_path: &'a str,
    url: &'a str,
    compression_type: Option<&'a str>,
    file_hash: &'a [u8],
    file_size: u64,
    nar_hash: &'a [u8],
    nar_size: u64,
    deriver: Option<&'a str>,
    system: &'a str,
    references: Vec<&'a str>,
}

impl<'a> NarInfo<'a> {
    pub fn new(
        store_path: &'a str,
        url: &'a str,
        file_size: u64,
        compression_type: Option<&'a str>,
        nar_hash: &'a [u8],
        nar_size: u64,
        deriver: Option<&'a str>,
        references: Vec<&'a str>,
    ) -> Self {
        Self {
            store_path: store_path,
            url: url,
            compression_type: compression_type,
            file_hash: &[],
            file_size: file_size,
            nar_hash: nar_hash,
            nar_size: nar_size,
            deriver: deriver,
            system: "", // TODO: Get system
            references: references,
        }
    }
}

impl<'a> Display for NarInfo<'a> {
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
        let file_size = self.file_size.to_string();
        let nar_size = self.nar_size.to_string();
        let values = [
            self.store_path,
            self.url,
            self.compression_type.unwrap_or(""),
            "",
            // &to_nix_base32(&self.file_hash),
            file_size.as_str(),
            "",
            // &to_nix_base32(&self.nar_hash),
            nar_size.as_str(),
            self.deriver.unwrap_or(""),
            self.system,
            &self.references.join(" "),
        ];
        for (key, value) in keys.iter().zip(values) {
            write!(f, "{}: {}\n", key, value)?;
        }
        Ok(())
    }
}

pub fn get_from_tree(cache: &GitRepo, key: &str) -> Result<Option<Vec<u8>>> {
    cache.get_blob(key, NARINFO_REF)
}

pub fn add_file(cache: &GitRepo, path: &PathBuf, key: &str) -> Result<()> {
    let content = fs::read(path)?;
    cache.add_file_content(key, &content, NARINFO_REF)?;
    Ok(())
}

pub fn exists(cache: &GitRepo, key: &str) -> bool {
    cache.query(key, NARINFO_REF).is_some()
}
pub fn list(cache: &GitRepo) -> Result<Vec<String>> {
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

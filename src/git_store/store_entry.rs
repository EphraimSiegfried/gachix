use crate::nar_info::NarInfo;
use anyhow::anyhow;
use regex::Regex;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use crate::git_store::GitStore;

use anyhow::Result;

const SUPER_REF: &str = "refs/SUPER";
const NARINFO_REF: &str = "refs/NARINFO";

pub fn get_as_nar(cache: &GitStore, key: &str) -> Result<Option<Vec<u8>>> {
    let mut nar = Vec::new(); // TODO: Implement as stream
    let res = cache.get_tree_as_nar(&mut nar, key, SUPER_REF)?;
    if res.is_none() {
        return Ok(None);
    }
    Ok(Some(nar))
}

pub fn add_entry(cache: &GitStore, path: &PathBuf) -> Result<()> {
    let key = get_hash_from_path(&path)?;

    let nar_info = NarInfo::new(key.to_string(), format!("nar/{}.nar", key.to_string()));
    let nar_info = nar_info.to_string();

    if cache.query(&key, NARINFO_REF).is_some() {
        return Ok(());
    }
    cache.add_file_content(&key, nar_info.as_bytes(), NARINFO_REF)?;
    if path.is_dir() {
        cache.add_dir(&key, path, SUPER_REF)?;
    } else {
        cache.add_file_content(&key, &fs::read(path)?, SUPER_REF)?;
    }
    Ok(())
}

fn get_hash_from_path(path: &PathBuf) -> Result<String> {
    let re = Regex::new(r"(?<hash>[0-9a-z]{32})").unwrap();
    let Some(caps) = re.captures(path.to_str().unwrap()) else {
        return Err(anyhow!(
            "Nix hash not found in path: {}",
            path.to_str().unwrap()
        ));
    };
    return Ok(caps["hash"].to_string());
}

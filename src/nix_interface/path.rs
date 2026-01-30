use anyhow::{Result, anyhow};
use std::{fmt::Display, path::Path};

#[derive(Debug, Clone)]
pub struct NixPath {
    path: String,
    hash: String,
    name: String,
}

impl NixPath {
    pub fn new<T: AsRef<Path> + ?Sized>(path_like: &T) -> Result<Self> {
        let path_ref = path_like.as_ref();
        let full_path = path_ref
            .to_str()
            .ok_or_else(|| anyhow!("Nix path is not valid UTF-8: {}", path_ref.display()))?;
        let full_path = full_path.trim();

        let stem = path_ref
            .file_name()
            .ok_or_else(|| anyhow!("Nix path has no file name component: {}", full_path))?;
        let stem_str = stem
            .to_str()
            .ok_or_else(|| anyhow!("Nix path component is not valid UTF-8: {}", full_path))?;

        let (hash, name) = stem_str.split_once('-').ok_or_else(|| {
            anyhow!(
                "Invalid nix path format (missing 'hash-name' separator): {}",
                stem_str
            )
        })?;

        if hash.len() != 32 {
            return Err(anyhow!("Invalid nix hash in nix path: {}", full_path));
        }

        Ok(Self {
            path: full_path.to_string(),
            hash: hash.to_string(),
            name: name.to_string(),
        })
    }

    pub fn get_base_32_hash(&self) -> &str {
        &self.hash
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_path(&self) -> &str {
        &self.path
    }
}

impl AsRef<str> for NixPath {
    fn as_ref(&self) -> &str {
        &self.path
    }
}

impl AsRef<Path> for NixPath {
    fn as_ref(&self) -> &Path {
        Path::new(&self.path)
    }
}

impl Display for NixPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.path)
    }
}
impl PartialEq for NixPath {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

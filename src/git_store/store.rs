use crate::nar::NarGitEncoder;
use crate::nar::decode::NarGitDecoder;
use anyhow::{Context, Result, anyhow};
use git2::{FileMode, Oid, Repository};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::RwLock;
use tracing::{Level, debug, info, span, trace};

pub struct GitStore {
    repo: RwLock<Repository>,
}
unsafe impl Sync for GitStore {}

impl GitStore {
    pub fn new(path_to_repo: &Path) -> Result<Self, git2::Error> {
        let repo = if path_to_repo.exists() {
            Repository::open(path_to_repo)?
        } else {
            Repository::init(path_to_repo)?
        };
        Ok(Self {
            repo: RwLock::new(repo),
        })
    }

    pub fn add_file_content(
        &self,
        key: &str,
        content: &[u8],
        tree_ref: &str,
    ) -> Result<(String, Oid)> {
        // return early if entry already exists
        if let Some(entry) = self.query(&key, tree_ref) {
            return Ok((key.to_string(), entry));
        }

        let read_repo = self.repo.read().unwrap();
        let blob_oid = read_repo.blob(content)?;
        drop(read_repo);

        self.update_tree(key, blob_oid, FileMode::Blob.into(), tree_ref)?;

        Ok((key.to_string(), blob_oid))
    }

    pub fn add_dir(&self, key: &str, path: &Path, tree_ref: &str) -> Result<(String, Oid)> {
        if let Some(entry) = self.query(&key, tree_ref) {
            return Ok((key.to_string(), entry));
        }
        let tree_oid = self.create_tree_from_dir(&path)?;
        self.update_tree(key, tree_oid, FileMode::Tree.into(), tree_ref)?;
        Ok((key.to_string(), tree_oid))
    }

    pub fn add_nar(&self, key: &str, content: impl Read, tree_ref: &str) -> Result<(String, Oid)> {
        let span = span!(Level::TRACE, "Add Nar", key);
        let _guard = span.enter();
        // return early if entry already exists
        if let Some(entry) = self.query(&key, tree_ref) {
            debug!("Cache Hit");
            return Ok((key.to_string(), entry));
        }

        let repo = self.repo.read().unwrap();
        let decoder = NarGitDecoder::new(&repo);
        trace!("Decoding NAR File");
        let (oid, filemode) = decoder
            .parse(content)
            .with_context(|| "Error decoding NAR file")?;
        drop(repo);

        self.update_tree(key, oid, filemode, tree_ref)?;
        Ok((key.to_string(), oid))
    }

    pub fn get_blob(&self, key: &str, tree_ref: &str) -> Result<Option<Vec<u8>>> {
        let repo = self.repo.read().unwrap();
        let Ok(tree_oid) = self.get_oid_from_reference(tree_ref) else {
            return Ok(None); // Maybe return error?
        };
        let tree = repo.find_tree(tree_oid)?;
        let Some(tree_entry) = tree.get_name(key) else {
            return Ok(None);
        };
        let object = tree_entry.to_object(&repo)?;
        let blob = object
            .into_blob()
            .map_err(|obj| anyhow!("Object was not a blob: {:?}", obj.kind()))?;
        Ok(Some(blob.content().to_vec()))
    }

    pub fn get_tree_as_nar(
        &self,
        result: &mut impl Write,
        key: &str,
        tree_ref: &str,
    ) -> Result<Option<()>> {
        let repo = self.repo.read().unwrap();
        let Ok(tree_oid) = self.get_oid_from_reference(tree_ref) else {
            return Ok(None);
        };

        let tree = repo.find_tree(tree_oid)?;
        let Some(tree_entry) = tree.get_name(key) else {
            return Ok(None);
        };

        let filemode = tree_entry.filemode();
        let object = tree_entry.to_object(&repo)?;
        let nar_encoder = NarGitEncoder::new(&repo, &object, filemode);
        nar_encoder.encode_into(result).map(|r| Some(r))
    }

    pub fn get_oid_from_reference(&self, tree_ref: &str) -> Result<Oid> {
        let repo = self.repo.read().unwrap();
        let reference = repo.find_reference(tree_ref)?;
        let tree = reference.peel_to_tree()?;
        Ok(tree.id())
    }

    fn create_tree_from_dir(&self, path: &Path) -> Result<Oid> {
        let repo = self.repo.read().unwrap();
        let mut builder = repo.treebuilder(None)?;

        for entry in path.read_dir()? {
            let entry_path = entry?.path();
            let entry_file_name = entry_path
                .file_name()
                .expect("Failed to get filename")
                .to_str()
                .unwrap();

            if entry_path.is_file() {
                let blob_oid = repo.blob_path(&entry_path)?;
                builder.insert(entry_file_name, blob_oid, FileMode::Blob.into())?;
            } else if entry_path.is_dir() {
                let subtree_oid = self.create_tree_from_dir(&entry_path)?;
                builder.insert(entry_file_name, subtree_oid, FileMode::Tree.into())?;
            }
        }
        Ok(builder.write()?)
    }

    fn update_tree(&self, name: &str, oid: Oid, mode: i32, tree_ref: &str) -> Result<Oid> {
        let span = span!(Level::TRACE, "Update Tree", name, tree_ref,);
        let _guard = span.enter();
        trace!("Trying to acquire write lock");
        let repo = self.repo.write().unwrap();

        // Retrieve last tree
        let tree = repo
            .find_reference(tree_ref)
            .ok()
            .and_then(|r| r.peel_to_tree().ok());

        if tree.is_none() {
            info!("Using empty tree for {tree_ref}");
        }

        let mut tree_builder = repo.treebuilder(tree.as_ref())?;

        trace!("Inserting object to tree");
        tree_builder.insert(name, oid, mode)?;
        let new_tree_oid = tree_builder.write()?;

        trace!("Updating reference");
        repo.reference(tree_ref, new_tree_oid, true, "")?;

        Ok(new_tree_oid)
    }

    pub fn query(&self, key: &str, tree_ref: &str) -> Option<Oid> {
        let repo = self.repo.read().unwrap();
        let Ok(tree_oid) = self.get_oid_from_reference(tree_ref) else {
            return None;
        };
        let tree = repo.find_tree(tree_oid).ok()?;
        tree.get_name(key).map(|entry| entry.id())
    }

    pub fn list_keys(&self, tree_ref: &str) -> Result<Vec<String>> {
        let repo = self.repo.read().unwrap();
        let tree_oid = self.get_oid_from_reference(tree_ref)?;
        let tree = repo.find_tree(tree_oid)?;
        let keys = tree.iter().map(|e| e.name().unwrap().to_string()).collect();
        Ok(keys)
    }
}

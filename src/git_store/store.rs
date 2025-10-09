use crate::nar::NarGitEncoder;
use git2::{FileMode, Oid, Repository, Signature, Time};
use nix_base32::to_nix_base32;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::RwLock;

pub struct GitStore {
    repo: RwLock<Repository>,
    head_tree_oid: RwLock<Option<Oid>>,
}
unsafe impl Sync for GitStore {}

impl GitStore {
    pub fn new(path_to_repo: &Path) -> Result<Self, git2::Error> {
        let repo = if path_to_repo.exists() {
            Repository::open(path_to_repo)?
        } else {
            Repository::init(path_to_repo)?
        };
        let head_tree_oid = Self::get_head_tree_id(&repo);

        Ok(Self {
            repo: RwLock::new(repo),
            head_tree_oid: RwLock::new(head_tree_oid),
        })
    }

    pub fn add(&self, path: &Path) -> Result<(String, Oid), git2::Error> {
        if path.is_dir() {
            self.add_dir(path)
        } else {
            self.add_file(path)
        }
    }

    fn add_file(&self, path: &Path) -> Result<(String, Oid), git2::Error> {
        let mut file = File::open(path).expect("Failed to open file");
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).expect("Failed to read file");

        let file_hash = base32_encode(&sha256_hash(&buffer));

        // return early if entry already exists
        if let Some(entry) = self.query(&file_hash) {
            return Ok((file_hash, entry));
        }

        let repo = self.repo.write().unwrap();
        let blob_oid = repo.blob(&buffer)?;

        self.update_tree_and_commit(&file_hash, blob_oid, FileMode::Blob)?;

        Ok((file_hash, blob_oid))
    }

    fn add_dir(&self, path: &Path) -> Result<(String, Oid), git2::Error> {
        let dir_name = path.file_name().unwrap().to_str().unwrap().to_string();

        // return early if entry already exists
        if let Some(entry) = self.query(&dir_name) {
            return Ok((dir_name, entry));
        }

        // create_tree_from_dir is an expensive call
        let dir_tree_oid = self.create_tree_from_dir(path)?;

        self.update_tree_and_commit(&dir_name, dir_tree_oid, FileMode::Tree)?;

        Ok((dir_name, dir_tree_oid))
    }

    pub fn get_nar(&self, key: &str) -> Result<Vec<u8>, std::io::Error> {
        let repo = self.repo.read().unwrap();
        let head_tree_oid = *self.head_tree_oid.read().unwrap();
        let head_tree = head_tree_oid.and_then(|oid| repo.find_tree(oid).ok());

        // TODO: Error handling
        let head_tree = head_tree.unwrap();
        let tree_entry = head_tree.get_name(key).unwrap();
        let filemode = tree_entry.filemode();
        let repo = self.repo.read().unwrap();
        let object = tree_entry.to_object(&repo).unwrap();
        let nar_encoder = NarGitEncoder::new(&repo, &object, filemode);
        nar_encoder.encode()
    }

    fn create_tree_from_dir(&self, path: &Path) -> Result<Oid, git2::Error> {
        let repo = self.repo.write().unwrap();
        let mut builder = repo.treebuilder(None)?;

        for entry in path.read_dir().expect("Failed to read directory") {
            let entry = entry.expect("Failed to get directory entry");
            let entry_path = entry.path();
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
        builder.write()
    }

    fn update_tree_and_commit(
        &self,
        name: &str,
        oid: Oid,
        mode: FileMode,
    ) -> Result<Oid, git2::Error> {
        let repo = self.repo.write().unwrap();
        // Retrieve head tree
        let head_tree_oid = *self.head_tree_oid.read().unwrap();
        let head_tree = head_tree_oid.and_then(|oid| repo.find_tree(oid).ok());

        // Create a tree builder based on last tree
        let mut tree_builder = repo.treebuilder(head_tree.as_ref())?;

        // Insert new entry into
        tree_builder.insert(name, oid, mode.into())?;
        let new_tree_oid = tree_builder.write()?;
        let new_tree = repo.find_tree(new_tree_oid)?;

        // Commit
        let sig = Signature::new("gachix", "gachix@gachix.com", &Time::new(0, 0))?;
        let commit_oid = repo.commit(None, &sig, &sig, "", &new_tree, &[])?;
        repo.set_head_detached(commit_oid)?;

        Ok(new_tree_oid)
    }

    pub fn query(&self, key: &str) -> Option<Oid> {
        let repo = self.repo.read().unwrap();
        let head_tree_oid = *self.head_tree_oid.read().unwrap();
        let head_tree = head_tree_oid.and_then(|oid| repo.find_tree(oid).ok())?;
        head_tree.get_name(key).map(|entry| entry.id())
    }

    pub fn list_keys(&self) -> Option<Vec<String>> {
        let repo = self.repo.read().unwrap();
        let head_tree_oid = *self.head_tree_oid.read().unwrap();
        let head_tree = head_tree_oid.and_then(|oid| repo.find_tree(oid).ok())?;
        let keys = head_tree
            .iter()
            .map(|e| e.name().unwrap().to_string())
            .collect();
        Some(keys)
    }

    fn get_head_tree_id(repo: &Repository) -> Option<Oid> {
        let head = repo.head().ok()?;
        let commit = head.peel_to_commit().ok()?;
        let last_tree = commit.tree().ok()?;
        Some(last_tree.id())
    }
}

fn sha256_hash(buf: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(buf);
    hasher.finalize().to_vec()
}

fn base32_encode(hash: &[u8]) -> String {
    to_nix_base32(hash)
}

use crate::nar::NarTreeEncoder;
use git2::{Commit, FileMode, Oid, Repository, Signature, Time, Tree};
use nix_base32::to_nix_base32;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub struct CaCache {
    repo: Repository,
}

impl CaCache {
    pub fn new(path_to_repo: &Path) -> Result<Self, git2::Error> {
        let repo = if path_to_repo.exists() {
            Repository::open(path_to_repo)?
        } else {
            Repository::init(path_to_repo)?
        };
        Ok(Self { repo })
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

        let blob_oid = self.repo.blob(&buffer)?;

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
        let t = self.last_tree().unwrap();
        let tree_entry = t.get_name(key).unwrap();
        let filemode = tree_entry.filemode();
        let object = tree_entry.to_object(&self.repo).unwrap();
        let nar_encoder = NarTreeEncoder::new(&self.repo, &object, filemode);
        nar_encoder.encode()
    }

    fn create_tree_from_dir(&self, path: &Path) -> Result<Oid, git2::Error> {
        let mut builder = self.repo.treebuilder(None)?;

        for entry in path.read_dir().expect("Failed to read directory") {
            let entry = entry.expect("Failed to get directory entry");
            let entry_path = entry.path();
            let entry_file_name = entry_path
                .file_name()
                .expect("Failed to get filename")
                .to_str()
                .unwrap();

            if entry_path.is_file() {
                let blob_oid = self.repo.blob_path(&entry_path)?;
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
        let parent_commit = self.repo.head().ok().and_then(|r| r.peel_to_commit().ok());
        let last_tree = parent_commit.as_ref().and_then(|commit| commit.tree().ok());

        let mut tree_builder = self.repo.treebuilder(last_tree.as_ref())?;

        tree_builder.insert(name, oid, mode.into())?;
        let tree_oid = tree_builder.write()?;
        let new_tree = self.repo.find_tree(tree_oid)?;

        let parents: Vec<&Commit> = parent_commit.as_ref().into_iter().collect();

        self.commit(&new_tree, &parents)?;

        Ok(tree_oid)
    }

    fn commit(&self, tree: &Tree, parents: &[&Commit]) -> Result<Oid, git2::Error> {
        // TODO: optimize by using once_cell
        let sig = Signature::new("gachix", "gachix@gachix.com", &Time::new(0, 0))?;
        self.repo
            .commit(Some("HEAD"), &sig, &sig, "", &tree, parents)
    }

    pub fn query(&self, key: &str) -> Option<Oid> {
        let t = self.last_tree()?;
        t.get_name(key).map(|entry| entry.id())
    }

    pub fn list_keys(&self) -> Option<Vec<String>> {
        self.last_tree()
            .and_then(|t| Some(t.iter().map(|e| e.name().unwrap().to_string()).collect()))
    }

    fn last_tree(&self) -> Option<Tree<'_>> {
        self.repo
            .head()
            .ok()
            .and_then(|r| r.peel_to_commit().ok().and_then(|c| c.tree().ok()))
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

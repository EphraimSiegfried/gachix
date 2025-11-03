use crate::nar::NarGitStream;
use crate::nar::decode::NarGitDecoder;
use anyhow::{Context, Result, anyhow};
use git2::Signature;
use git2::Time;
use git2::{FileMode, Oid, Repository};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{Level, debug, info, span, trace};

pub struct GitRepo {
    repo: Arc<RwLock<Repository>>,
}
unsafe impl Sync for GitRepo {}
unsafe impl Send for GitRepo {}

impl GitRepo {
    pub fn new(path_to_repo: &Path) -> Result<Self, git2::Error> {
        let repo = if path_to_repo.exists() {
            Repository::open(path_to_repo)?
        } else {
            Repository::init(path_to_repo)?
        };
        Ok(Self {
            repo: RwLock::new(repo).into(),
        })
    }

    pub fn add_file_content(&self, key: &str, content: &[u8], tree_ref: &str) -> Result<Oid> {
        // return early if entry already exists
        if let Some(entry) = self.query(&key, tree_ref) {
            return Ok(entry);
        }

        let read_repo = self.repo.read().unwrap();
        let blob_oid = read_repo.blob(content)?;
        drop(read_repo);

        self.update_tree(key, blob_oid, FileMode::Blob.into(), tree_ref)?;

        Ok(blob_oid)
    }

    #[allow(dead_code)]
    pub fn add_dir<T: AsRef<Path>>(&self, key: &str, path: &T, tree_ref: &str) -> Result<Oid> {
        let path = path.as_ref();
        if !path.is_dir() {
            return Err(anyhow!("No such directory: {}", path.to_str().unwrap()));
        }
        if let Some(entry) = self.query(&key, tree_ref) {
            return Ok(entry);
        }
        let tree_oid = self.create_tree_from_dir(&path)?;
        self.update_tree(key, tree_oid, FileMode::Tree.into(), tree_ref)?;
        Ok(tree_oid)
    }

    pub fn add_nar(&self, key: &str, content: impl Read, tree_ref: &str) -> Result<Oid> {
        let span = span!(Level::TRACE, "Add Nar", key);
        let _guard = span.enter();
        // return early if entry already exists
        if let Some(entry) = self.query(&key, tree_ref) {
            debug!("Cache Hit");
            return Ok(entry);
        }

        let repo = self.repo.read().unwrap();
        let decoder = NarGitDecoder::new(&repo);
        trace!("Decoding NAR File");
        let (oid, filemode) = decoder
            .parse(content)
            .with_context(|| "Error decoding NAR file")?;
        drop(repo);

        self.update_tree(key, oid, filemode, tree_ref)?;
        Ok(oid)
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

    pub fn get_tree_as_nar_stream(
        &self,
        key: &str,
        tree_ref: &str,
    ) -> Result<Option<NarGitStream>> {
        let repo = self.repo.read().unwrap();
        let Ok(tree_oid) = self.get_oid_from_reference(tree_ref) else {
            return Ok(None);
        };

        let tree = repo.find_tree(tree_oid)?;
        let Some(tree_entry) = tree.get_name(key) else {
            return Ok(None);
        };

        let filemode = tree_entry.filemode();
        let oid = tree_entry.id();

        // let repo_cloned = repo.clone();
        let repo_owned = Arc::clone(&self.repo);
        let stream = NarGitStream::new(repo_owned, oid, filemode);
        Ok(Some(stream))
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

            if entry_path.is_symlink() {
                let target = fs::read_link(&entry_path)?;
                let blob_oid = repo.blob(target.as_os_str().as_bytes())?;
                builder.insert(entry_file_name, blob_oid, FileMode::Link.into())?;
            } else if entry_path.is_file() {
                let permissions = entry_path.metadata()?.permissions();
                let is_executable = permissions.mode() & 0o111 != 0;
                let filemode = if is_executable {
                    FileMode::BlobExecutable
                } else {
                    FileMode::Blob
                };
                let blob_oid = repo.blob_path(&entry_path)?;
                builder.insert(entry_file_name, blob_oid, filemode.into())?;
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

    pub fn commit(&self, tree_oid: Oid, parent_oids: &[Oid], comment: Option<&str>) -> Result<Oid> {
        let sig = Signature::new("gachix", "gachix@gachix.com", &Time::new(0, 0))?;
        let repo = self.repo.write().unwrap();
        let commit_tree = repo.find_tree(tree_oid)?;
        let mut parents: Vec<git2::Commit<'_>> = Vec::new();
        for oid in parent_oids.iter() {
            let commit = repo.find_commit(*oid)?;
            parents.push(commit);
        }
        let parents: Vec<&git2::Commit<'_>> = parents.iter().collect();

        let commit_oid = repo.commit(
            None,
            &sig,
            &sig,
            comment.unwrap_or(""),
            &commit_tree,
            parents.as_slice(),
        )?;
        Ok(commit_oid)
    }

    pub fn get_tree_to_commit_map(&self) -> Result<HashMap<Oid, Oid>> {
        let repo = self.repo.read().unwrap();
        let mut tree_to_commit = HashMap::new();
        repo.odb()?.foreach(|oid| {
            if let Ok(commit) = repo.find_commit(oid.clone()) {
                let tree_oid = commit.tree().unwrap().id();
                tree_to_commit.insert(tree_oid, oid.clone());
            }
            true
        })?;
        Ok(tree_to_commit)
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
        let Ok(tree_oid) = self.get_oid_from_reference(tree_ref) else {
            return Ok(Vec::new());
        };
        let tree = repo.find_tree(tree_oid)?;
        let keys = tree.iter().map(|e| e.name().unwrap().to_string()).collect();
        Ok(keys)
    }
}

impl Clone for GitRepo {
    fn clone(&self) -> Self {
        Self {
            repo: self.repo.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use rand::distributions::{Alphanumeric, DistString};
    use rand::{self};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_random_package(dir: &PathBuf) -> Result<PathBuf> {
        let mut rng = rand::thread_rng();
        let random_string = Alphanumeric.sample_string(&mut rng, 5);
        let package_path = dir.join(&random_string);
        fs::create_dir(&package_path)?;
        fs::write(package_path.join("some_file"), random_string)?;
        Ok(package_path.to_path_buf())
    }

    #[test]
    fn test_tree_to_commit_map() -> Result<()> {
        let temp = TempDir::new()?;
        let repo_path = temp.path().join("test_repo");
        let repo = GitRepo::new(&repo_path)?;
        let t1 = repo.add_dir("1", &create_random_package(&repo_path)?, "refs/some_ref")?;
        let t2 = repo.add_dir("2", &create_random_package(&repo_path)?, "refs/some_ref")?;
        let c1 = repo.commit(t1, &[], None)?;
        let c2 = repo.commit(t2, &[], None)?;
        let actual = repo.get_tree_to_commit_map()?;

        let mut expected = HashMap::new();
        expected.insert(t1, c1);
        expected.insert(t2, c2);

        assert_eq!(expected, actual);
        Ok(())
    }
}

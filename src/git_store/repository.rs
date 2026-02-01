use crate::nar::NarGitStream;
use crate::nar::decode::NarGitDecoder;
use anyhow::{Context, Result, anyhow, bail};
use git2::Cred;
use git2::Direction;
use git2::FetchOptions;
use git2::RemoteCallbacks;
use git2::Signature;
use git2::Time;
use git2::{ErrorCode, FileMode, Oid, Repository};
use std::env;
use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{Level, info, instrument, span, trace};

pub struct GitRepo {
    repo: Arc<RwLock<Repository>>,
}
unsafe impl Sync for GitRepo {}
unsafe impl Send for GitRepo {}

impl GitRepo {
    pub fn new(path_to_repo: &Path) -> Result<Self, git2::Error> {
        let repo = if path_to_repo.exists() {
            info!(
                "Using an existing Git repository at {}",
                path_to_repo.to_str().unwrap()
            );
            Repository::open(path_to_repo)?
        } else {
            info!(
                "Initializing a new Git repository at {}",
                path_to_repo.to_str().unwrap()
            );
            Repository::init(path_to_repo)?
        };
        let mut config = repo.config()?;
        config.set_str("protocol.version", "2")?;
        Ok(Self {
            repo: RwLock::new(repo).into(),
        })
    }

    pub fn add_file_content(&self, content: &[u8]) -> Result<Oid> {
        let read_repo = self.repo.read().unwrap();
        let blob_oid = read_repo.blob(content)?;
        Ok(blob_oid)
    }

    pub fn add_single_entry_tree(&self, entry_oid: Oid, name: &str, filemode: i32) -> Result<Oid> {
        let repo = self.repo.read().unwrap();
        let mut builder = repo.treebuilder(None)?;
        builder.insert(&name, entry_oid, filemode)?;
        Ok(builder.write()?)
    }

    #[allow(dead_code)]
    pub fn add_dir<T: AsRef<Path>>(&self, path: &T) -> Result<Oid> {
        let path = path.as_ref();
        if !path.is_dir() {
            return Err(anyhow!("No such directory: {}", path.to_str().unwrap()));
        }
        let tree_oid = self.create_tree_from_dir(&path)?;
        Ok(tree_oid)
    }

    pub fn add_nar(&self, content: impl Read) -> Result<(Oid, i32)> {
        let repo = self.repo.read().unwrap();
        let decoder = NarGitDecoder::new(&repo);
        let (oid, filemode) = decoder
            .parse(content)
            .with_context(|| "Error decoding NAR file")?;
        Ok((oid, filemode))
    }

    pub fn get_blob(&self, oid: Oid) -> Result<Vec<u8>> {
        let repo = self.repo.read().unwrap();
        let blob = repo.find_blob(oid)?;
        Ok(blob.content().to_vec())
    }

    pub fn add_ref(&self, ref_name: &str, oid: Oid) -> Result<()> {
        let repo = self.repo.read().unwrap();
        repo.reference(&ref_name, oid, false, "")?;
        Ok(())
    }

    pub fn get_entry_as_nar(&self, oid: Oid) -> Result<Option<NarGitStream>> {
        let repo = self.repo.read().unwrap();
        let object = repo.find_object(oid, None)?;
        let kind = object
            .kind()
            .ok_or_else(|| anyhow!("Object with oid {} does not have a type", oid))?;
        let filemode = match kind {
            git2::ObjectType::Blob => FileMode::Blob.into(),
            git2::ObjectType::Tree => FileMode::Tree.into(),
            _ => bail!("Object must either be a tree or a blob"),
        };

        let repo_owned = Arc::clone(&self.repo);
        let stream = NarGitStream::new(repo_owned, oid, filemode);
        Ok(Some(stream))
    }

    pub fn get_oid_from_reference(&self, reference: &str) -> Option<Oid> {
        let repo = self.repo.read().unwrap();
        let res = repo.find_reference(reference).ok().and_then(|r| r.target());
        res
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

    pub fn commit(&self, tree_oid: Oid, parent_oids: &[Oid], comment: Option<&str>) -> Result<Oid> {
        let span = span!(Level::TRACE, "Commiting", comment);
        let _guard = span.enter();

        let repo = self.repo.write().unwrap();
        let sig = Signature::new("gachix", "gachix@gachix.com", &Time::new(0, 0))?;

        trace!("Retrieving main tree object {}", tree_oid);
        let commit_tree = repo.find_tree(tree_oid)?;

        trace!("Collecting commit oids for {} parents", parent_oids.len());
        let mut parents: Vec<git2::Commit<'_>> = Vec::new();
        for oid in parent_oids.iter() {
            trace!("Retrieving parent commit {}", oid);
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
        trace!("Commit successful");
        Ok(commit_oid)
    }

    pub fn reference_exists(&self, name: &str) -> Result<bool> {
        let repo = self.repo.read().unwrap();
        match repo.find_reference(name) {
            Ok(_) => Ok(true),
            Err(e) => {
                if e.code() == ErrorCode::NotFound {
                    Ok(false)
                } else {
                    bail!(e)
                }
            }
        }
    }

    pub fn list_references(&self, ref_name: &str) -> Result<Vec<String>> {
        let repo = self.repo.read().unwrap();
        let refs = repo.references_glob(ref_name)?;
        let mut refs_names = Vec::new();
        for reference in refs {
            let reference = reference?;
            refs_names.push(
                reference
                    .name()
                    .ok_or_else(|| anyhow!("Could not get name from reference"))?
                    .to_string(),
            );
        }
        Ok(refs_names)
    }

    pub fn match_sole_entry_id(&self, tree_oid: Oid, name: &str) -> Result<Option<Oid>> {
        let repo = self.repo.read().unwrap();
        let tree = repo.find_tree(tree_oid)?;
        if tree.len() != 1 {
            return Ok(None);
        }
        let entry = tree
            .iter()
            .next()
            .ok_or_else(|| anyhow!("Expected at least one entry in tree"))?;
        let entry_oid = entry.name().and_then(|n| {
            if n == name {
                return Some(entry.id());
            }
            None
        });
        Ok(entry_oid)
    }

    pub fn check_remote_health(&self, url: &str) -> Result<()> {
        let repo = self.repo.read().unwrap();
        let mut remote = repo.remote_anonymous(url)?;
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(|_url, _user_from_url, _allowed_types| {
            let user = env::var("USER").unwrap();
            if _allowed_types.contains(git2::CredentialType::USERNAME) {
                return git2::Cred::username(&user);
            }
            Cred::ssh_key(
                &env::var("USER").unwrap(),
                None,
                std::path::Path::new(&format!("{}/.ssh/id_ed25519", env::var("HOME").unwrap())),
                None,
            )
        });
        match remote.connect_auth(Direction::Fetch, Some(callbacks), None) {
            Ok(connection) => {
                connection.list()?;
                Ok(())
            }
            Err(e) => {
                bail!("Connection failed: {}", e);
            }
        }
    }

    #[instrument(skip(self))]
    pub fn fetch(&self, url: &str, reference: &str) -> Result<Option<()>> {
        let repo = self.repo.read().unwrap();
        let mut remote = match repo.find_remote("peer") {
            Ok(remote) => remote,
            _ => repo.remote_with_fetch("peer", url, "")?,
        };
        let refspec = format!("{}:{}", reference, reference);

        trace!("Fetching from remote");
        let mut fetch_options = FetchOptions::new();
        let mut callbacks = RemoteCallbacks::new();
        callbacks.update_tips(|r, _, _| {
            trace!("Added reference {r}");
            true
        });
        callbacks.credentials(|_url, _user_from_url, _allowed_types| {
            let user = env::var("USER").unwrap();
            if _allowed_types.contains(git2::CredentialType::USERNAME) {
                return git2::Cred::username(&user);
            }
            Cred::ssh_key(
                &env::var("USER").unwrap(),
                None,
                std::path::Path::new(&format!("{}/.ssh/id_ed25519", env::var("HOME").unwrap())),
                None,
            )
        });
        fetch_options.remote_callbacks(callbacks);
        fetch_options.download_tags(git2::AutotagOption::None);
        fetch_options.update_fetchhead(false);
        remote.fetch(&vec![refspec], Some(&mut fetch_options), None)?;

        if remote.stats().received_objects() == 0 {
            trace!("Did not receive anything");
            return Ok(None);
        }
        trace!("Received {} objects", remote.stats().received_objects());

        Ok(Some(()))
    }
}

impl Clone for GitRepo {
    fn clone(&self) -> Self {
        Self {
            repo: self.repo.clone(),
        }
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use anyhow::Result;
//     use rand::distributions::{Alphanumeric, DistString};
//     use rand::{self};
//     use std::fs;
//     use std::path::PathBuf;
//     use tempfile::TempDir;
//
//     fn create_random_package(dir: &PathBuf) -> Result<PathBuf> {
//         let mut rng = rand::thread_rng();
//         let random_string = Alphanumeric.sample_string(&mut rng, 5);
//         let package_path = dir.join(&random_string);
//         fs::create_dir(&package_path)?;
//         fs::write(package_path.join("some_file"), random_string)?;
//         Ok(package_path.to_path_buf())
//     }
// }

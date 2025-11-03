use crate::nar::NarGitStream;
use crate::nix_interface::daemon::AsyncStream;
use crate::nix_interface::daemon::NixDaemon;
use crate::nix_interface::nar_info::NarInfo;
use crate::nix_interface::path::NixPath;
use anyhow::anyhow;
use anyhow::bail;
use git2::Oid;
use nix_base32::from_nix_base32;
use nix_base32::to_nix_base32;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

use crate::git_store::GitRepo;

use anyhow::Result;
const SUPER_REF: &str = "refs/SUPER";
const NARINFO_REF: &str = "refs/NARINFO";

// nix_hash -> (tree_oid, commit_oid)
type Index = HashMap<Vec<u8>, (Oid, Oid)>;

#[derive(Clone)]
pub struct Store {
    repo: GitRepo,
    object_index: Arc<RwLock<Index>>,
}

impl Store {
    pub fn new(repo: GitRepo) -> Result<Self> {
        debug!("Computing Object Index");
        let tree_to_commit = repo.get_tree_to_commit_map()?;
        let hash_to_tree = get_hash_to_tree_map(&repo)?;
        let object_index: HashMap<Vec<u8>, (Oid, Oid)> = hash_to_tree
            .iter()
            .map(|(hash, oid)| {
                let key = hash.clone();
                let value = (*oid, *tree_to_commit.get(oid).unwrap());
                (key, value)
            })
            .collect();
        info!("Repository contains {} packages", object_index.len());
        Ok(Self {
            repo,
            object_index: Arc::new(RwLock::new(object_index)),
        })
    }

    pub async fn add_closure(&self, store_path: &NixPath, count: usize) -> Result<Oid> {
        if count == 100 {
            bail!("Dependency Depth Limit exceeded");
        }
        let index = self.object_index.read().unwrap();
        if let Some((_, commit_oid)) = index.get(&store_path.get_hash_bytes()) {
            debug!("Package already exists: {}", store_path.get_name());
            return Ok(*commit_oid);
        }
        drop(index);

        let mut nix = NixDaemon::local().await?;
        let (narinfo, tree_oid) = self.try_add_package(&mut nix, store_path).await?;
        drop(nix);

        let deps = narinfo.get_dependencies();
        let mut parent_commits = Vec::new();
        for dependency in deps {
            let dep_coid = Box::pin(self.add_closure(&dependency, count + 1)).await?;
            parent_commits.push(dep_coid);
        }
        let commit_oid =
            self.repo
                .commit(tree_oid, &parent_commits, Some(store_path.get_name()))?;

        let mut index = self.object_index.write().unwrap();
        index.insert(store_path.get_hash_bytes(), (tree_oid, commit_oid));

        Ok(commit_oid)
    }

    async fn try_add_package(
        &self,
        nix: &mut NixDaemon<impl AsyncStream>,
        store_path: &NixPath,
    ) -> Result<(NarInfo, Oid)> {
        info!("Adding package: {}", store_path.get_name());
        let path_exists = nix.path_exists(&store_path).await?;
        if !path_exists {
            // TODO: try to build package if it does not exist
            return Err(anyhow!("Path does not exist {}", store_path));
        }
        let narinfo = self
            .add_narinfo(nix, store_path.get_base_32_hash(), store_path)
            .await?;
        let reader = nix.fetch(store_path)?;
        let tree_oid = self
            .repo
            .add_nar(store_path.get_base_32_hash(), reader, SUPER_REF)?;

        Ok((narinfo, tree_oid))
    }

    async fn add_narinfo(
        &self,
        nix: &mut NixDaemon<impl AsyncStream>,
        key: &str,
        store_path: &NixPath,
    ) -> Result<NarInfo> {
        let Some(path_info) = nix.get_pathinfo(&store_path).await? else {
            return Err(anyhow!(
                "Could not find narinfo for {}",
                store_path.get_path()
            ));
        };
        let deriver = path_info.deriver.map(|d| NixPath::new(&d)).transpose()?;
        let refs_result: Result<Vec<NixPath>, anyhow::Error> = path_info
            .references
            .iter()
            .map(|p| NixPath::new(p))
            .collect();

        let narinfo = NarInfo::new(
            store_path.clone(),
            key.to_string(),
            0,
            None,
            "".to_string(),
            path_info.nar_size,
            deriver,
            refs_result?,
        );

        self.repo
            .add_file_content(key, narinfo.to_string().as_bytes(), NARINFO_REF)?;
        Ok(narinfo)
    }

    pub fn entry_exists(&self, base32_hash: &str) -> Result<bool> {
        let index = self.object_index.read().unwrap();
        let digest =
            from_nix_base32(base32_hash).ok_or_else(|| anyhow!("Invalid key {}", base32_hash))?;
        Ok(index.contains_key(&digest))
    }

    pub fn get_as_nar_stream(&self, key: &str) -> Result<Option<NarGitStream>> {
        self.repo.get_tree_as_nar_stream(key, SUPER_REF)
    }

    pub fn get_narinfo(&self, base32_hash: &str) -> Result<Option<Vec<u8>>> {
        self.repo.get_blob(&base32_hash, NARINFO_REF)
    }

    pub fn list_entries(&self) -> Vec<String> {
        let index = self.object_index.read().unwrap();
        index.keys().map(|k| to_nix_base32(k)).collect()
    }
}

fn get_hash_to_tree_map(repo: &GitRepo) -> Result<HashMap<Vec<u8>, Oid>> {
    let available_nix_hashes = repo.list_keys(NARINFO_REF)?;

    let mut hash_to_tree = HashMap::new();
    for base32_hash in available_nix_hashes {
        let narinfo_bytes = repo.get_blob(&base32_hash, NARINFO_REF)?.unwrap();
        let narinfo_str = String::from_utf8(narinfo_bytes)?;
        let narinfo = NarInfo::parse(&narinfo_str)?;
        let tree_oid = repo
            .query(&narinfo.key, SUPER_REF)
            .ok_or_else(|| anyhow!("Key in narinfo does not point to a valid package"))?;
        let hash = from_nix_base32(&base32_hash)
            .ok_or_else(|| anyhow!("Invalid base 32 hash: {}", base32_hash))?;
        hash_to_tree.insert(hash, tree_oid);
    }
    Ok(hash_to_tree)
}

#[cfg(test)]
mod tests {
    use crate::{
        git_store::{
            GitRepo,
            store::{NARINFO_REF, SUPER_REF, Store, get_hash_to_tree_map},
        },
        nix_interface::{daemon::NixDaemon, nar_info::NarInfo, path::NixPath},
    };
    use anyhow::{Result, anyhow};
    use std::{collections::HashMap, path::Path, process::Command};
    use tempfile::TempDir;

    fn build_nix_package(package_name: &str) -> Result<NixPath> {
        let output = Command::new("nix")
            .arg("build")
            .arg(format!("nixpkgs#{}", package_name))
            .arg("--print-out-paths")
            .output()?;

        let path = NixPath::new(&String::from_utf8_lossy(&output.stdout).to_string())?;
        Ok(path)
    }

    #[test]
    fn test_hash_tree_hashmap() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().join("gachix");
        let repo = GitRepo::new(&repo_path)?;

        let packages = vec!["hello", "kitty"];
        let mut expected = HashMap::new();
        for pkg in packages {
            let path = build_nix_package(pkg)?;
            let oid = repo.add_dir(path.get_base_32_hash(), &Path::new("result"), SUPER_REF)?;

            let narinfo = NarInfo::new(
                path.clone(),
                path.get_base_32_hash().to_string(),
                0,
                None,
                "lol".to_string(),
                0,
                None,
                Vec::new(),
            );
            repo.add_file_content(
                path.get_base_32_hash(),
                &narinfo.to_string().as_bytes(),
                NARINFO_REF,
            )?;

            expected.insert(path.get_hash_bytes(), oid);
        }
        let actual = get_hash_to_tree_map(&repo)?;
        assert_eq!(actual, expected);
        Ok(())
    }

    #[tokio::test]
    async fn test_add_package() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().join("gachix");
        let repo = GitRepo::new(&repo_path)?;
        let store = Store::new(repo)?;

        let path = build_nix_package("hello")?;
        let mut nix = NixDaemon::local().await?;
        store.try_add_package(&mut nix, &path).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_add_closure() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().join("gachix");
        let repo = GitRepo::new(&repo_path)?;
        let store = Store::new(repo)?;

        let path = build_nix_package("sl")?;
        store.add_closure(&path, 0).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_add_narinfo() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().join("gachix");
        let repo = GitRepo::new(&repo_path)?;
        let store = Store::new(repo)?;

        let path = build_nix_package("kitty")?;
        let mut nix = NixDaemon::local().await?;
        store.add_narinfo(&mut nix, "key", &path).await?;
        let narinfo = store
            .get_narinfo("key")?
            .ok_or_else(|| anyhow!("Could not get narinfo"))?;
        let narinfo = NarInfo::parse(&String::from_utf8_lossy(&narinfo))?;
        assert_eq!(narinfo.store_path, path);
        Ok(())
    }
}

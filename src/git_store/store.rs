use std::collections::HashSet;
use std::collections::VecDeque;

use crate::nar::NarGitStream;
use crate::nix_interface::daemon::AsyncStream;
use crate::nix_interface::daemon::NixDaemon;
use crate::nix_interface::nar_info::NarInfo;
use crate::nix_interface::path::NixPath;
use crate::settings;
use anyhow::{anyhow, bail};
use git2::Oid;
use tracing::instrument;
use tracing::{debug, info, trace, warn};

use crate::git_store::GitRepo;

use anyhow::Result;

#[derive(Clone)]
pub struct Store {
    settings: settings::Store,
    repo: GitRepo,
}

impl Store {
    pub fn new(settings: settings::Store) -> Result<Self> {
        let repo = GitRepo::new(&settings.path)?;
        let entries = repo.list_references("{PACKGAGE_PREFIX_REF}/*")?;
        info!("Repository contains {} packages", entries.len());
        Ok(Self { settings, repo })
    }

    pub async fn peer_health_check(&self) -> bool {
        let mut success = true;

        match NixDaemon::local().await {
            Ok(_) => info!("Succesfully connected to local Nix daemon"),
            Err(e) => {
                success = false;
                warn!("Failed to connect to remote local daemon: {e}")
            }
        }

        for host_name in &self.settings.builders {
            match NixDaemon::remote(host_name, 22).await {
                Ok(_) => info!("Succesfully connected to Nix daemon at {host_name}"),
                Err(e) => {
                    success = false;
                    warn!("Failed to connect to remote Nix daemon at {host_name}: {e}",)
                }
            };
        }

        for git_remote in &self.settings.remotes {
            match self.repo.check_remote_health(&git_remote) {
                Ok(_) => info!("Succesfully connected to Git repository at {git_remote}"),
                Err(e) => {
                    success = false;
                    warn!("Failed to connect to Git repository {git_remote}: {e}")
                }
            }
        }

        success
    }

    pub async fn add_closure(&self, store_path: &NixPath) -> Result<()> {
        info!("Adding closure for {}", store_path.get_name());
        let (_, num_packages_added) = self._add_closure(store_path, 0).await?;
        info!("Added {num_packages_added} packages");
        Ok(())
    }

    pub async fn _add_closure(&self, store_path: &NixPath, count: usize) -> Result<(Oid, usize)> {
        info!("Adding package: {}", store_path.get_name());
        if count == 100 {
            bail!("Dependency Depth Limit exceeded");
        }
        let package_id = store_path.get_base_32_hash();
        if let Some(commit_oid) = self.get_commit(package_id) {
            debug!("Package already exists: {}", store_path.get_name());
            return Ok((commit_oid, 0));
        }
        if let Some(commit_oid) = self.add_package_from_git_remotes(store_path)? {
            debug!(
                "Package retrieved from remote Git peer: {}",
                store_path.get_name()
            );
            return Ok((commit_oid, 0));
        }

        let mut nix = NixDaemon::local().await?;
        let (narinfo, tree_oid) = self.try_add_package(&mut nix, store_path).await?;
        drop(nix);

        let deps = narinfo.get_dependencies();
        let mut parent_commits = Vec::new();
        let mut total_packages_added = 0;
        for dependency in &deps {
            let (dep_coid, num_packages_added) =
                Box::pin(self._add_closure(&dependency, count + 1)).await?;
            total_packages_added += num_packages_added;
            parent_commits.push(dep_coid);
        }
        let commit_oid =
            self.repo
                .commit(tree_oid, &parent_commits, Some(store_path.get_name()))?;

        self.repo
            .add_ref(&self.get_result_ref(package_id), commit_oid)?;
        Ok((commit_oid, 1 + total_packages_added))
    }

    pub fn get_commit(&self, hash: &str) -> Option<Oid> {
        self.repo.get_oid_from_reference(&self.get_result_ref(hash))
    }

    fn get_package_ref(&self, hash: &str) -> String {
        format!("refs/{hash}")
    }

    fn get_result_ref(&self, hash: &str) -> String {
        format!("{}/result", self.get_package_ref(hash))
    }

    fn get_narinfo_ref(&self, hash: &str) -> String {
        format!("{}/narinfo", self.get_package_ref(hash))
    }

    fn add_package_from_git_remotes(&self, store_path: &NixPath) -> Result<Option<Oid>> {
        let package_id = store_path.get_base_32_hash();
        let mut commit_oid = None;
        let mut success_remote = "";
        for remote in &self.settings.remotes {
            if let Some(oid) = self.add_package_from_remote(package_id, remote)? {
                commit_oid = Some(oid);
                success_remote = &remote;
                break;
            }
        }
        if commit_oid == None {
            return Ok(None);
        }

        let mut open = VecDeque::new();
        let mut visited = HashSet::new();
        open.push_back(package_id.to_string());
        visited.insert(package_id.to_string());
        while let Some(id) = open.pop_front() {
            for dep in self.get_dep_ids(&id)? {
                if !visited.contains(&dep) {
                    if !(self.repo.reference_exists(&self.get_result_ref(&dep))?
                        && self.repo.reference_exists(&self.get_narinfo_ref(&dep))?)
                    {
                        self.add_package_from_remote(&dep, success_remote)?;
                    }
                    // TODO: do I need to add to open queue if references already exist?
                    open.push_back(dep.clone());
                    visited.insert(dep.clone());
                }
            }
        }

        Ok(commit_oid)
    }

    fn add_package_from_remote(&self, package_id: &str, remote: &str) -> Result<Option<Oid>> {
        if let Some(()) = self
            .repo
            .fetch(&remote, &format!("{}/*", self.get_package_ref(package_id)))?
        {
            let oid = self
                .repo
                .get_oid_from_reference(&self.get_result_ref(package_id))
                .ok_or_else(|| anyhow!("Could not get commit id for {}", package_id))?;
            return Ok(Some(oid));
        }
        Ok(None)
    }

    fn get_dep_ids(&self, package_id: &str) -> Result<Vec<String>> {
        let narinfo_ref = self.get_narinfo_ref(package_id);
        let narinfo_oid = self
            .repo
            .get_oid_from_reference(&narinfo_ref)
            .ok_or_else(|| anyhow!("Could not find narinfo from reference {}", narinfo_ref))?;
        let narinfo_blob = self.repo.get_blob(narinfo_oid)?;
        let narinfo = NarInfo::parse(&String::from_utf8_lossy(&narinfo_blob).to_string())?;
        let dependencies = narinfo.get_dependencies();
        let ids = dependencies
            .iter()
            .map(|d| d.get_base_32_hash().to_string())
            .collect::<Vec<String>>();
        Ok(ids)
    }

    async fn try_add_package(
        &self,
        nix: &mut NixDaemon<impl AsyncStream>,
        store_path: &NixPath,
    ) -> Result<(NarInfo, Oid)> {
        let path_exists = nix.path_exists(&store_path).await?;
        if !path_exists {
            // TODO: try to build package if it does not exist
            return Err(anyhow!("Path does not exist {}", store_path));
        }

        trace!("Fetching package content");
        let reader = nix.fetch(store_path)?;

        trace!("Adding package content to repository");
        let (entry_oid, _) = self.repo.add_nar(reader)?;

        trace!("Adding narinfo entry");
        let narinfo = self
            .add_narinfo(nix, &entry_oid.to_string(), store_path)
            .await?;

        Ok((narinfo, entry_oid))
    }

    #[instrument(skip(self, nix))]
    async fn add_narinfo(
        &self,
        nix: &mut NixDaemon<impl AsyncStream>,
        package_key: &str,
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
            package_key.to_string(),
            0,
            None,
            "".to_string(),
            path_info.nar_size,
            deriver,
            refs_result?,
        );

        debug!("Adding narinfo");
        let blob_oid = self.repo.add_file_content(narinfo.to_string().as_bytes())?;
        self.repo.add_ref(
            &self.get_narinfo_ref(store_path.get_base_32_hash()),
            blob_oid,
        )?;
        Ok(narinfo)
    }

    pub fn entry_exists(&self, base32_hash: &str) -> Result<bool> {
        self.repo
            .reference_exists(&self.get_result_ref(base32_hash))
    }

    pub fn get_as_nar_stream(&self, key: &str) -> Result<Option<NarGitStream>> {
        self.repo.get_entry_as_nar(Oid::from_str(key)?)
    }

    pub fn get_narinfo(&self, base32_hash: &str) -> Result<Option<Vec<u8>>> {
        let result = self
            .repo
            .get_oid_from_reference(&self.get_narinfo_ref(base32_hash));
        match result {
            Some(oid) => Ok(Some(self.repo.get_blob(oid)?)),
            None => Ok(None),
        }
    }

    pub fn list_entries(&self) -> Result<Vec<String>> {
        let entries = self.repo.list_references("{PACKGAGE_PREFIX_REF}/*")?;
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        git_store::{GitRepo, store::Store},
        nix_interface::{daemon::NixDaemon, nar_info::NarInfo, path::NixPath},
        settings,
    };
    use anyhow::{Result, anyhow};
    use std::path::PathBuf;
    use std::process::Command;
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

    pub fn set_repo_path(path: &PathBuf) -> settings::Store {
        settings::Store {
            path: path.clone(),
            builders: vec![],
            remotes: vec![],
            use_local_nix_daemon: true,
        }
    }

    #[tokio::test]
    async fn test_add_package() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().join("gachix");
        let store = Store::new(set_repo_path(&repo_path))?;

        let path = build_nix_package("hello")?;
        let mut nix = NixDaemon::local().await?;
        store.try_add_package(&mut nix, &path).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_add_closure() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().join("gachix");
        let store = Store::new(set_repo_path(&repo_path))?;

        let path = build_nix_package("sl")?;
        store.add_closure(&path).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_add_narinfo() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().join("gachix");
        let store = Store::new(set_repo_path(&repo_path))?;

        let path = build_nix_package("kitty")?;
        let mut nix = NixDaemon::local().await?;
        store.add_narinfo(&mut nix, "key", &path).await?;
        let narinfo = store
            .get_narinfo(path.get_base_32_hash())?
            .ok_or_else(|| anyhow!("Could not get narinfo"))?;
        let narinfo = NarInfo::parse(&String::from_utf8_lossy(&narinfo))?;
        assert_eq!(narinfo.store_path, path);
        Ok(())
    }
}

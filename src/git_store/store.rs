use std::collections::HashSet;
use std::collections::VecDeque;
use std::fs;
use std::str::FromStr;

use crate::git_store::GitRepo;
use crate::nar::NarGitStream;
use crate::nix_interface::daemon::DynNixDaemon;
use crate::nix_interface::daemon::NixDaemon;
use crate::nix_interface::nar_info::NarInfo;
use crate::nix_interface::path::NixPath;
use crate::nix_interface::signature::PrivateKey;
use crate::nix_interface::signature::fingerprint_store_object;
use crate::settings;
use anyhow::{anyhow, bail};
use async_recursion::async_recursion;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use git2::Oid;
use tracing::field::debug;
use tracing::{debug, info, warn};

use anyhow::Result;

#[derive(Clone)]
pub struct Store {
    settings: settings::Store,
    repo: GitRepo,
    private_key: Option<PrivateKey>,
}

impl Store {
    pub fn new(settings: settings::Store) -> Result<Self> {
        let repo = GitRepo::new(&settings.path)?;

        let private_key = if let Some(key_path) = &settings.sign_private_key_path {
            Some(PrivateKey::from_str(&fs::read_to_string(key_path)?)?)
        } else {
            None
        };

        let store = Self {
            settings,
            repo,
            private_key,
        };
        info!(
            "Repository contains {} packages",
            store.num_available_packages()?
        );
        Ok(store)
    }

    pub fn available_daemons(&self) -> Result<Vec<DynNixDaemon>> {
        let mut daemons = Vec::new();
        if self.settings.use_local_nix_daemon {
            daemons.push(DynNixDaemon::Local(NixDaemon::local()));
        }
        for url in &self.settings.builders {
            daemons.push(DynNixDaemon::Remote(NixDaemon::remote(
                &url.host_str().unwrap(),
            )));
        }
        Ok(daemons)
    }

    pub async fn peer_health_check(&self) -> bool {
        let mut success = true;

        for mut daemon in self.available_daemons().unwrap() {
            match daemon.connect().await {
                Ok(_) => info!(
                    "Succesfully connected to Nix daemon at {}",
                    daemon.get_address()
                ),
                Err(e) => {
                    success = false;
                    warn!(
                        "Failed to connect to remote Nix daemon at {} : {}",
                        daemon.get_address(),
                        e
                    )
                }
            };
            daemon.disconnect();
        }

        for url in &self.settings.remotes {
            let url_str = url.as_str();
            let host = url.host().unwrap();
            match self.repo.check_remote_health(&url_str) {
                Ok(_) => info!("Succesfully connected to Git repository at {}", host),
                Err(e) => {
                    success = false;
                    warn!("Failed to connect to Git repository {}: {}", host, e)
                }
            }
        }

        success
    }

    pub async fn add_single(&self, package_path: &NixPath) -> Result<()> {
        info!("Adding single package {}", package_path.get_name());
        let package_id = package_path.get_base_32_hash();

        let narinfo_ref = self.get_narinfo_ref(package_id);

        if self.repo.reference_exists(&narinfo_ref)? {
            debug!("Package already exists");
            return Ok(());
        }

        let Ok(Some((_, narinfo_blob_oid, _))) =
            self.get_package_from_nix_daemons(package_path).await
        else {
            bail!(
                "There doesn't exist a Nix daemon which has {}",
                package_path
            );
        };
        self.repo.add_ref(&narinfo_ref, narinfo_blob_oid)?;
        Ok(())
    }

    pub async fn add_closure(&self, package_path: &NixPath) -> Result<()> {
        info!("Adding closure for {}", package_path.get_name());
        let entries_before = self.num_available_packages()?;
        match self._add_closure(package_path).await? {
            Some(_) => {
                let entries_after = self.num_available_packages()?;
                let num_packages_added = entries_after - entries_before;
                info!("Added {num_packages_added} packages")
            }
            None => bail!(
                "Could not add closure of package {}",
                package_path.get_name()
            ),
        }
        Ok(())
    }

    #[async_recursion]
    pub async fn _add_closure(&self, package_path: &NixPath) -> Result<Option<Oid>> {
        let package_id = package_path.get_base_32_hash();

        // Check if commit already exists locally
        if let Some(commit_oid) = self.get_commit(package_id) {
            debug!("Package already exists: {}", package_path.get_name());
            return Ok(Some(commit_oid));
        }

        // Ask Git peers if they have replicated the package
        if let Some(commit_oid) = self.get_package_commit_from_git_remotes(package_path)? {
            return Ok(Some(commit_oid));
        }

        // Ask known Nix daemons if they can build the package
        let Ok(Some((narinfo, narinfo_blob_oid, package_oid))) =
            self.get_package_from_nix_daemons(package_path).await
        else {
            return Ok(None);
        };

        // Recurse into package dependecies and collect their commit oids
        let deps = narinfo.get_dependencies();
        let mut parent_commits = Vec::new();
        for dependency in &deps {
            let Some(dep_coid) = self._add_closure(&dependency).await? else {
                return Ok(None);
            };
            parent_commits.push(dep_coid);
        }

        // Commit the package tree and specify dependency commits as parents
        let commit_oid =
            self.repo
                .commit(package_oid, &parent_commits, Some(package_path.get_name()))?;

        // Add references: nix-hash -> package-commit-oid, nix-hash -> narinfo-blob-oid
        self.repo
            .add_ref(&self.get_result_ref(package_id), commit_oid)?;
        self.repo
            .add_ref(&self.get_narinfo_ref(package_id), narinfo_blob_oid)?;
        Ok(Some(commit_oid))
    }

    pub async fn get_package_from_nix_daemons(
        &self,
        package_path: &NixPath,
    ) -> Result<Option<(NarInfo, Oid, Oid)>> {
        for mut daemon in self.available_daemons()? {
            daemon.connect().await?;
            // Ask if daemon has the package
            // TODO: ask it to build the package if it does not have it
            if !daemon.path_exists(package_path).await? {
                continue;
            };
            // Add the package contents to the Git database
            let clone = self.repo.clone();
            let package_oid = daemon
                .fetch(package_path, move |r| {
                    let (oid, _) = clone.add_nar(r)?;
                    Ok(oid)
                })
                .await?;

            // Get metadata info about the package and add it to the Git database
            let narinfo = self
                .build_narinfo(&mut daemon, package_oid.to_string().as_str(), package_path)
                .await?;
            let narinfo_blob_oid = self.repo.add_file_content(narinfo.to_string().as_bytes())?;

            match &daemon {
                DynNixDaemon::Local(_) => {
                    debug!("Using local daemon, fetched {} ", package_path.get_name())
                }
                DynNixDaemon::Remote(daemon) => debug!(
                    "Using daemon at {}, fetched package {}",
                    daemon.get_address(),
                    package_path.get_name()
                ),
            }
            daemon.disconnect();
            return Ok(Some((narinfo, narinfo_blob_oid, package_oid)));
        }
        Ok(None)
    }

    fn get_package_commit_from_git_remotes(&self, store_path: &NixPath) -> Result<Option<Oid>> {
        let package_id = store_path.get_base_32_hash();
        let mut commit_oid = None;
        let mut success_remote = "";
        for remote_url in &self.settings.remotes {
            let url = remote_url.as_str();
            if let Some(oid) = self.fetch_from_remote(package_id, url)? {
                debug!(
                    "Using git peer at {}, fetched package {}",
                    remote_url,
                    store_path.get_name()
                );
                commit_oid = Some(oid);
                success_remote = url;
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
                let dep_hash = dep.get_base_32_hash();
                if !visited.contains(dep_hash) {
                    if !(self.repo.reference_exists(&self.get_result_ref(dep_hash))?
                        && self
                            .repo
                            .reference_exists(&self.get_narinfo_ref(dep_hash))?)
                    {
                        self.fetch_from_remote(dep_hash, success_remote)?;
                        debug!(
                            "Using git peer at {}, fetched package {}",
                            success_remote,
                            dep.get_name()
                        );
                    }
                    // TODO: do I need to add to open queue if references already exist?
                    open.push_back(dep_hash.to_string());
                    visited.insert(dep_hash.to_string());
                }
            }
        }

        Ok(commit_oid)
    }

    fn fetch_from_remote(&self, package_id: &str, remote: &str) -> Result<Option<Oid>> {
        if let Some(()) = self
            .repo
            .fetch(&remote, &format!("{}/*", self.get_package_ref(package_id)))?
        {
            let oid = self
                .get_commit(package_id)
                .ok_or_else(|| anyhow!("Could not get commit id for {}", package_id))?;
            return Ok(Some(oid));
        }
        Ok(None)
    }

    fn get_dep_ids(&self, package_id: &str) -> Result<Vec<NixPath>> {
        let narinfo_blob = self
            .get_narinfo(package_id)?
            .ok_or_else(|| anyhow!("Could not find narinfo for {}", package_id))?;
        let narinfo = NarInfo::parse(&String::from_utf8_lossy(&narinfo_blob).to_string())?;
        let dependencies = narinfo.get_dependencies();
        Ok(dependencies.into_iter().cloned().collect())
    }

    async fn build_narinfo(
        &self,
        nix_daemon: &mut DynNixDaemon,
        key: &str,
        store_path: &NixPath,
    ) -> Result<NarInfo> {
        let Some(path_info) = nix_daemon.get_pathinfo(&store_path).await? else {
            return Err(anyhow!(
                "Could not find narinfo for {}",
                store_path.get_path()
            ));
        };
        let references: Vec<NixPath> = path_info
            .references
            .iter()
            .map(|p| NixPath::new(p))
            .collect::<Result<Vec<_>, _>>()?;

        let nar_size = path_info.nar_size;
        let nar_hash = hex::decode(path_info.nar_hash)?;

        // TODO: compute hash instead of copying it and verify it against the received hash
        let mut nar_hash_32_base = nix_base32::to_nix_base32(&nar_hash);
        // TODO: formatting should be handled by the NarInfo struct
        nar_hash_32_base = format!("sha256:{}", nar_hash_32_base);

        let signature = self.private_key.as_ref().map(|private_key| {
            let fingerprint =
                fingerprint_store_object(store_path, &nar_hash_32_base, nar_size, &references);
            let signature_bytes = private_key.sign(fingerprint.as_bytes());
            format!(
                "{}:{}",
                private_key.name,
                BASE64_STANDARD.encode(signature_bytes)
            )
        });

        let deriver = path_info.deriver.map(|d| NixPath::new(&d)).transpose()?;
        let narinfo = NarInfo::new(
            store_path.clone(),
            key.to_string(),
            nar_hash_32_base.clone(),
            path_info.nar_size,
            None,
            nar_hash_32_base,
            path_info.nar_size,
            deriver,
            references,
            signature,
        );
        Ok(narinfo)
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

    pub fn entry_exists(&self, base32_hash: &str) -> Result<bool> {
        self.repo
            .reference_exists(&self.get_result_ref(base32_hash))
    }

    pub fn get_as_nar_stream(&self, key: &str) -> Result<Option<NarGitStream>> {
        self.repo.get_entry_as_nar(Oid::from_str(key)?)
    }

    pub fn list_entries(&self) -> Result<Vec<String>> {
        let entries = self.repo.list_references("refs/*")?;
        Ok(entries)
    }

    fn num_available_packages(&self) -> Result<usize> {
        Ok(self.repo.list_references("refs/*/narinfo")?.len())
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
}

#[cfg(test)]
mod tests {
    use crate::{
        git_store::store::Store,
        nix_interface::{
            daemon::{DynNixDaemon, NixDaemon},
            path::NixPath,
        },
        settings,
    };
    use anyhow::Result;
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
            sign_private_key_path: None,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_package() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().join("gachix");
        let store = Store::new(set_repo_path(&repo_path))?;

        let path = build_nix_package("hello")?;
        store.get_package_from_nix_daemons(&path).await?;
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
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
        let mut nix = DynNixDaemon::Local(NixDaemon::local());
        nix.connect().await?;
        store.build_narinfo(&mut nix, "somekey", &path).await?;
        Ok(())
    }
}

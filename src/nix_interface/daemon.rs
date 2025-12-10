use std::collections::HashMap;
use std::io::{BufReader, Read};

use anyhow::{Result, anyhow, bail};
use async_ssh2_lite::{AsyncChannel, AsyncSession, TokioTcpStream};
use futures::io;
use nix_daemon::{BuildMode, ClientSettings, Progress, Store, nix::DaemonStore};
use nix_daemon::{BuildResult, PathInfo};
use std::net::ToSocketAddrs;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio_util::io::SyncIoBridge;

use crate::nix_interface::path::NixPath;

pub trait AsyncStream: AsyncWriteExt + AsyncReadExt + Unpin + Unpin + Send {}
impl<T> AsyncStream for T where T: AsyncWriteExt + AsyncReadExt + AsyncWrite + Unpin + Send {}

pub struct NixDaemon<C: AsyncStream> {
    daemon: Option<DaemonStore<C>>,
    address: String,
}

impl NixDaemon<UnixStream> {
    pub fn local() -> Self {
        Self {
            daemon: None,
            address: "/nix/var/nix/daemon-socket/socket".to_string(),
        }
    }
    pub async fn connect(&mut self) -> Result<()> {
        let store = DaemonStore::builder().connect_unix(&self.address).await?;
        self.daemon = Some(store);
        Ok(())
    }
}
impl NixDaemon<AsyncChannel<TokioTcpStream>> {
    pub fn remote(address: &str) -> Self {
        Self {
            daemon: None,
            address: address.to_string(),
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        let addr = (self.address.as_str(), 22)
            .to_socket_addrs()?
            .next()
            .ok_or(anyhow!("Failed to resolve address"))?;
        let stream = TokioTcpStream::connect(addr).await?;
        let mut session = AsyncSession::new(stream, None)?;
        session.handshake().await?;

        let home = dirs::home_dir().ok_or(anyhow!("Home directory not found"))?;
        let key = home.join(".ssh").join("id_ed25519");
        let user = whoami::username();

        session
            .userauth_pubkey_file(&user, None, &key, None)
            .await?;
        if !session.authenticated() {
            return Err(anyhow!("Could not authenticate to remote",));
        }
        let mut channel = session.channel_session().await?;
        channel.exec("nix daemon --stdio").await?;
        self.daemon = Some(DaemonStore::builder().init(channel).await?);
        Ok(())
    }
}

impl<C: AsyncStream> NixDaemon<C> {
    pub async fn get_pathinfo(&mut self, path: &NixPath) -> Result<Option<PathInfo>> {
        let Some(daemon) = &mut self.daemon else {
            bail!("Not connected to Nix Daemon")
        };
        let path_info = daemon.query_pathinfo(path).result().await?;
        Ok(path_info)
    }

    pub async fn build(&mut self, drv_paths: &[&NixPath]) -> Result<HashMap<String, BuildResult>> {
        let Some(daemon) = &mut self.daemon else {
            bail!("Not connected to Nix Daemon")
        };
        daemon.set_options(ClientSettings {
            try_fallback: true,
            use_substitutes: false,
            ..ClientSettings::default()
        });
        let out_drv_paths = drv_paths.iter().map(|p| format!("{}!out", p));
        let result = daemon
            .build_paths_with_results(out_drv_paths, BuildMode::Normal)
            .result()
            .await?;
        Ok(result)
    }

    pub async fn path_exists(&mut self, store_path: &NixPath) -> Result<bool> {
        let Some(daemon) = &mut self.daemon else {
            bail!("Not connected to Nix Daemon")
        };
        let exists = daemon.is_valid_path(store_path).result().await?;
        Ok(exists)
    }

    pub async fn fetch<F, R>(&mut self, store_path: &NixPath, parser: F) -> Result<R>
    where
        R: Send + Sync + 'static,
        F: for<'a> FnOnce(&'a mut dyn Read) -> Result<R> + Send + Sync + 'static,
    {
        let Some(daemon) = &mut self.daemon else {
            bail!("Not connected to Nix Daemon")
        };

        let progress = daemon.nar_from_path(store_path, |reader| {
            Box::pin(async move {
                tokio::task::block_in_place(|| {
                    let sync_reader = SyncIoBridge::new(reader);
                    let mut buf_reader = BufReader::new(sync_reader);
                    let val = parser(&mut buf_reader)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    Ok(val)
                })
            })
        });

        let val = progress.result().await?;

        Ok(val)
    }
    pub fn get_address(&self) -> String {
        self.address.clone()
    }

    pub fn disconnect(mut self) {
        self.daemon = None;
    }
}

pub enum DynNixDaemon {
    Local(NixDaemon<UnixStream>),
    Remote(NixDaemon<AsyncChannel<TokioTcpStream>>),
}

impl DynNixDaemon {
    pub async fn connect(&mut self) -> Result<()> {
        match self {
            DynNixDaemon::Local(daemon) => daemon.connect().await,
            DynNixDaemon::Remote(daemon) => daemon.connect().await,
        }
    }

    pub async fn get_pathinfo(&mut self, path: &NixPath) -> Result<Option<PathInfo>> {
        match self {
            DynNixDaemon::Local(daemon) => daemon.get_pathinfo(path).await,
            DynNixDaemon::Remote(daemon) => daemon.get_pathinfo(path).await,
        }
    }

    pub async fn build(&mut self, drv_paths: &[&NixPath]) -> Result<HashMap<String, BuildResult>> {
        match self {
            DynNixDaemon::Local(daemon) => daemon.build(drv_paths).await,
            DynNixDaemon::Remote(daemon) => daemon.build(drv_paths).await,
        }
    }

    pub async fn path_exists(&mut self, store_path: &NixPath) -> Result<bool> {
        match self {
            DynNixDaemon::Local(daemon) => daemon.path_exists(store_path).await,
            DynNixDaemon::Remote(daemon) => daemon.path_exists(store_path).await,
        }
    }

    pub async fn fetch<F, R>(&mut self, store_path: &NixPath, parser: F) -> Result<R>
    where
        R: Send + Sync + 'static,
        F: for<'a> FnOnce(&'a mut dyn Read) -> Result<R> + Send + Sync + 'static,
    {
        match self {
            DynNixDaemon::Local(daemon) => daemon.fetch(store_path, parser).await,
            DynNixDaemon::Remote(daemon) => daemon.fetch(store_path, parser).await,
        }
    }

    pub fn disconnect(self) {
        match self {
            DynNixDaemon::Local(daemon) => daemon.disconnect(),
            DynNixDaemon::Remote(daemon) => daemon.disconnect(),
        }
    }

    pub fn get_address(&self) -> String {
        match self {
            DynNixDaemon::Local(daemon) => daemon.get_address(),
            DynNixDaemon::Remote(daemon) => daemon.get_address(),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use nix_daemon::BuildResultStatus;
    use rand;
    use std::io::Write;
    use std::process::Stdio;

    #[tokio::test]
    #[ignore]
    async fn test_connect_remote() -> Result<()> {
        let mut nix = NixDaemon::remote("blinkybill");
        nix.connect().await?;

        nix.get_pathinfo(&NixPath::new(
            "/nix/store/h0b3pxg56bh5lnh4bqrb2gsrbkdzmpsh-kitty-0.43.1",
        )?)
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_local_build_package() -> Result<()> {
        let mut nix = NixDaemon::local();
        nix.connect().await?;
        let drv_path = create_random_derivation().await?;
        let drv_path = NixPath::new(&drv_path)?;

        let result = nix.build(&[&drv_path]).await?;

        let key = format!("{}!out", drv_path);
        let build_result = result
            .get(&key)
            .ok_or_else(|| anyhow!("Did not find build result"))?;
        assert_eq!(build_result.status, BuildResultStatus::Built);

        Ok(())
    }

    async fn create_random_derivation() -> Result<String> {
        let cookie = {
            use rand::distributions::{Alphanumeric, DistString};
            Alphanumeric.sample_string(&mut rand::thread_rng(), 16)
        };

        let mut nix_instantiate = std::process::Command::new("nix-instantiate")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Couldn't spawn nix-instantiate");

        std::thread::spawn({
            let mut stdin = nix_instantiate.stdin.take().unwrap();
            let input = format!(
                "derivation {{
                    name = \"test_build_paths_{}\";
                    builder = \"/bin/sh\";
                    args = [ \"-c\" \"echo -n $name > $out\" ];
                    system = builtins.currentSystem;
                }}",
                cookie,
            );
            move || stdin.write_all(input.as_bytes())
        });
        let nix_instantiate_output = nix_instantiate
            .wait_with_output()
            .expect("nix-instantiate failed");
        Ok(String::from_utf8(nix_instantiate_output.stdout)
            .unwrap()
            .trim()
            .to_string())
    }
}

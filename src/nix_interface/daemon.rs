use std::collections::HashMap;
use std::io::Read;

use anyhow::{Result, anyhow};
use async_ssh2_lite::{AsyncChannel, AsyncSession, TokioTcpStream};
use nix_daemon::{BuildMode, ClientSettings, Progress, Store, nix::DaemonStore};
use nix_daemon::{BuildResult, PathInfo};
use nix_nar::Encoder;
use std::net::ToSocketAddrs;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::nix_interface::path::NixPath;

pub trait AsyncStream: AsyncWriteExt + AsyncReadExt + Unpin + Unpin + Send {}
impl<T> AsyncStream for T where T: AsyncWriteExt + AsyncReadExt + AsyncWrite + Unpin + Send {}

pub struct NixDaemon<C: AsyncStream> {
    store: DaemonStore<C>,
}

impl NixDaemon<UnixStream> {
    pub async fn local() -> Result<Self> {
        let store = DaemonStore::builder()
            .connect_unix("/nix/var/nix/daemon-socket/socket")
            .await?;
        Ok(Self { store })
    }
}
impl NixDaemon<AsyncChannel<TokioTcpStream>> {
    pub async fn remote(host: &str, port: u16) -> Result<Self> {
        let addr = (host, port)
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
        let store = DaemonStore::builder().init(channel).await?;
        Ok(Self { store })
    }
}

impl<C: AsyncStream> NixDaemon<C> {
    pub async fn get_pathinfo(&mut self, path: &NixPath) -> Result<Option<PathInfo>> {
        let path_info = self.store.query_pathinfo(path).result().await?;
        Ok(path_info)
    }

    pub async fn build(&mut self, drv_paths: &[&NixPath]) -> Result<HashMap<String, BuildResult>> {
        self.store.set_options(ClientSettings {
            try_fallback: true,
            use_substitutes: false,
            ..ClientSettings::default()
        });
        let out_drv_paths = drv_paths.iter().map(|p| format!("{}!out", p));
        let result = self
            .store
            .build_paths_with_results(out_drv_paths, BuildMode::Normal)
            .result()
            .await?;
        Ok(result)
    }

    pub async fn path_exists(&mut self, store_path: &NixPath) -> Result<bool> {
        let exists = self.store.is_valid_path(store_path).result().await?;
        Ok(exists)
    }

    pub fn fetch(&self, store_path: &NixPath) -> Result<impl Read> {
        let enc = Encoder::new(&store_path)?;
        Ok(enc)
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
    async fn test_connect_remote() -> Result<()> {
        let mut nix = NixDaemon::remote("blinkybill", 22).await?;

        nix.get_pathinfo(&NixPath::new(
            "/nix/store/h0b3pxg56bh5lnh4bqrb2gsrbkdzmpsh-kitty-0.43.1",
        )?)
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_local_build_package() -> Result<()> {
        let mut nix = NixDaemon::local().await?;
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

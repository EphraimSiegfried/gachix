use anyhow::{Result, anyhow, bail};
use assert_cmd;
use regex::Regex;
use reqwest;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

pub const NIXPGKS_VERSION: &str = "github:NixOS/nixpkgs/21.05";

pub struct CacheServer {
    child: Child,
}
impl CacheServer {
    pub fn start(port: u16, cache_path: &Path) -> Result<Self> {
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!());
        let mut child = command
            .env("GACHIX__STORE__PATH", cache_path)
            .env("GACHIX__SERVER__PORT", port.to_string())
            .arg("serve")
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| anyhow!("Failed to start server: {}", e))?;

        let base_url = format!("http://localhost:{}", port);
        let health_check_url = format!("{base_url}/nix-cache-info");

        let max_attempts = 10;
        let delay = Duration::from_millis(500);

        for attempt in 0..max_attempts {
            if let Some(_) = child.try_wait()? {
                bail!("Server stopped shortly after being spawned");
            }

            match reqwest::blocking::get(&health_check_url) {
                Ok(response) => {
                    if response.status().is_success() {
                        println!("Server ready after {} attempts.", attempt + 1);
                        return Ok(CacheServer { child });
                    }
                }
                Err(_) => {}
            }

            sleep(delay);
        }

        child.kill().unwrap_or_default(); // Clean up the server process
        let _ = child.wait();
        bail!(
            "Server failed to become ready after {} attempts ({}s timeout).",
            max_attempts,
            max_attempts as f64 * delay.as_secs_f64()
        );
    }
}

impl Drop for CacheServer {
    fn drop(&mut self) {
        match self.child.kill() {
            Ok(_) => println!("Successfully terminated server process."),
            Err(e) => eprintln!("Warning: Failed to terminate server process: {}", e),
        }
        let _ = self.child.wait();
    }
}
pub fn build_nix_package(package_name: &str) -> Result<PathBuf> {
    let output = Command::new("nix")
        .arg("build")
        .arg(format!("{}#{}", NIXPGKS_VERSION, package_name))
        .arg("--no-link")
        .arg("--print-out-paths")
        .output()?;

    let path = &String::from_utf8_lossy(&output.stdout).to_string();
    Ok(PathBuf::from(path))
}

pub fn delete_nix_package(package_name: &str) -> Result<()> {
    Command::new("nix")
        .arg("store")
        .arg("delete")
        .arg(format!("{}#{}", NIXPGKS_VERSION, package_name))
        .spawn()?;
    Ok(())
}

pub fn add_to_cache(
    store_path: &Path,
    cache_path: &Path,
    config: Option<HashMap<&str, &str>>,
) -> Result<()> {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!());
    let process = cmd
        .env_clear()
        .env("GACHIX__STORE__PATH", cache_path)
        .arg("add")
        .arg(store_path)
        .stdout(Stdio::null());
    if let Some(config) = config {
        process.envs(config);
    }
    let mut child = process.spawn()?;
    let status = child.wait()?;
    if !status.success() {
        bail!("Failed to add path to cache");
    }
    Ok(())
}

pub fn request(url: &str) -> Result<reqwest::blocking::Response> {
    let response = reqwest::blocking::get(url)?;
    assert!(
        response.status().is_success(),
        "Request failed for: {}. Server responded with status code {}",
        url,
        response.status()
    );
    return Ok(response);
}
pub fn get_hash(store_path: &Path) -> Result<String> {
    Ok(Regex::new(r"([0-9a-z]{32})")?
        .find(store_path.to_str().unwrap())
        .unwrap()
        .as_str()
        .to_string())
}

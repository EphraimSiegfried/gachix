use anyhow::{Result, anyhow, bail};
use assert_cmd::cargo::cargo_bin;
use reqwest;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread::sleep;
use std::time::Duration;

pub struct CacheServer {
    child: Child,
}
impl CacheServer {
    pub fn start(port: u16, cache_path: &Path) -> Result<Self> {
        let mut command = Command::new(cargo_bin!());
        let mut child = command
            .arg("--store-path")
            .arg(cache_path)
            .arg("serve")
            .arg("--port")
            .arg(port.to_string())
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
        .arg(format!("nixpkgs#{}", package_name))
        .arg("--print-out-paths")
        .output()?;

    let path = &String::from_utf8_lossy(&output.stdout).to_string();
    Ok(PathBuf::from(path))
}

pub fn add_to_cache(store_path: &Path, cache_path: &Path) -> Result<()> {
    let mut child = Command::new(cargo_bin!())
        .arg("--store-path")
        .arg(cache_path)
        .arg("add")
        .arg(store_path)
        .stdout(Stdio::null())
        .spawn()?;
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

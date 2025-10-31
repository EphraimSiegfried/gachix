mod common;
use anyhow::{Result, bail};
use bytes::Buf;
use nix_nar::Decoder;
use regex::Regex;
use reqwest::blocking::Response;
use reqwest::blocking::get;
use tempfile::TempDir;

#[test]
fn test_cache_info() -> Result<()> {
    let tempdir = TempDir::new()?;
    let temp_path = tempdir.path();
    let port = 9234;
    let base_url = format!("http://localhost:{}", port);

    let _server = common::CacheServer::start(port, &temp_path.join("gachix"))?;

    let response = get(&format!("{base_url}/nix-cache-info"))?;

    assert!(
        response.status().is_success(),
        "Server responded with unexpected status code {}",
        response.status()
    );

    let body = response.text()?;

    let pattern = Regex::new(r"^StoreDir: (/\w*)*\nWantMassQuery: (0|1)\nPriority: \d*$")?;
    assert!(
        pattern.is_match(&body),
        "Response body did not match the required regex pattern.\nBody received:\n{}",
        body
    );

    Ok(())
}

#[test]
fn test_package_retrieval() -> Result<()> {
    let tempdir = TempDir::new()?;
    let temp_path = tempdir.path();
    let port = 9234;
    let base_url = format!("http://localhost:{}", port);
    let repo_path = &temp_path.join("gachix");

    // start the server
    let _server = common::CacheServer::start(port, &repo_path)?;

    // Add some package to the cache
    let store_path = common::build_nix_package("hello")?;
    common::add_to_cache(&store_path, &repo_path)?;

    // retrieve nix hash from the nix path
    let nix_hash = Regex::new(r"([0-9a-z]{32})")?
        .find(store_path.to_str().unwrap())
        .unwrap()
        .as_str();

    let narinfo_response = common::request(&format!("{base_url}/{nix_hash}.narinfo"))?;
    let narinfo_body = narinfo_response.text()?;

    // retrieve URL value from narinfo
    let re = Regex::new(r"URL: (nar\/.*)\n")?;
    let Some(caps) = re.captures(&narinfo_body) else {
        bail!("Could not find URL in narinfo");
    };
    let url = &caps[1];

    // fetch the package
    let package_response = common::request(&format!("{base_url}/{url}"))?;

    // check whether the returned nar can be unpacked
    let package_nar = package_response.bytes()?;
    let decoder = Decoder::new(package_nar.reader())?;
    let package_path = temp_path.join("my_package");
    decoder.unpack(package_path)?;

    Ok(())
}

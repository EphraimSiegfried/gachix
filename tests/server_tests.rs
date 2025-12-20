mod common;
use std::{collections::HashMap, fs, process::Command};

use anyhow::{Result, bail};
use bytes::Buf;
use nix_nar::Decoder;
use regex::Regex;
use reqwest::StatusCode;
use tempfile::TempDir;

use crate::common::NIXPGKS_VERSION;

#[test]
fn test_cache_info_request() -> Result<()> {
    let tempdir = TempDir::new()?;
    let temp_path = tempdir.path();
    let port = 9234;
    let base_url = format!("http://localhost:{}", port);

    let _server = common::CacheServer::start(port, &temp_path.join("gachix"))?;

    let response = common::request(&format!("{base_url}/nix-cache-info"))?;

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
fn test_head_request() -> Result<()> {
    let tempdir = TempDir::new()?;
    let temp_path = tempdir.path();
    let port = 9231;
    let base_url = format!("http://localhost:{}", port);
    let repo_path = &temp_path.join("gachix");

    let store_path = common::build_nix_package("hello")?;
    common::add_to_cache(&store_path, &repo_path, None)?;
    let nix_hash = common::get_hash(&store_path)?;

    let _server = common::CacheServer::start(port, &repo_path)?;
    let url = format!("{base_url}/{nix_hash}.narinfo");

    let client = reqwest::blocking::Client::new();
    let response = client.head(&url).send()?;
    assert!(
        response.status() == StatusCode::OK,
        "Expected successful HEAD request, but it failed with status code: {}",
        response.status()
    );

    let url = format!("{base_url}/h0b3pxg56bh5lnh4bqrb2gsrbkdzmpsh.narinfo");
    let response = client.head(&url).send()?;
    assert!(
        response.status() == StatusCode::NOT_FOUND,
        "Expected status code NOT FOUND, but got: {}",
        response.status()
    );

    Ok(())
}

#[test]
fn test_narinfo_request() -> Result<()> {
    let tempdir = TempDir::new()?;
    let temp_path = tempdir.path();
    let port = 9238;
    let base_url = format!("http://localhost:{}", port);
    let repo_path = &temp_path.join("gachix");

    let store_path = common::build_nix_package("hello")?;
    common::add_to_cache(&store_path, &repo_path, None)?;
    let nix_hash = common::get_hash(&store_path)?;

    let _server = common::CacheServer::start(port, &temp_path.join("gachix"))?;

    let narinfo_response = common::request(&format!("{base_url}/{nix_hash}.narinfo"))?;
    let body = narinfo_response.text()?;

    // TODO: make more strict narinfo patterns
    let patterns = [
        r"^StorePath: [/\w]+[-\w\.]+",
        r"^URL: [/\w\.]+",
        r"^Compression:[ \w]*",
        r"^FileHash:[ \w:]",
        r"^FileSize: [\d]+",
        r"^NarHash:[ \w:]+",
        r"^NarSize: [\d]+",
        r"^References:[ /\w]+[-\w\. ]*",
        r"^Deriver: [ /\w]+[-\w\. ]*.drv",
        r"^Sig: [\w\.\-:=]*",
    ];

    let lines = body.lines();

    for (pattern, line) in patterns.iter().zip(lines) {
        let regex = Regex::new(pattern)?;
        assert!(
            regex.is_match(line),
            "NarInfo line did not match regex.\nExpected pattern: {}\nInstead got: {}",
            pattern,
            line
        );
    }

    Ok(())
}

#[test]
fn test_package_retrieval() -> Result<()> {
    let tempdir = TempDir::new()?;
    let temp_path = tempdir.path();
    let port = 9239;
    let base_url = format!("http://localhost:{}", port);
    let repo_path = &temp_path.join("gachix");

    // Add some package to the cache
    let store_path = common::build_nix_package("hello")?;
    common::add_to_cache(&store_path, &repo_path, None)?;

    // start the server
    let _server = common::CacheServer::start(port, &repo_path)?;

    // retrieve nix hash from the nix path
    let nix_hash = common::get_hash(&store_path)?;

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

#[test]
fn test_nix_substituter() -> Result<()> {
    let tempdir = TempDir::new()?;
    let temp_path = tempdir.path();
    let port = 9239;
    let base_url = format!("http://localhost:{}", port);
    let repo_path = &temp_path.join("gachix");

    // Fetch a package to Nix store
    let package_name = "lolcat";
    let store_path = common::build_nix_package(package_name)?;

    // Create signatures
    let key_name = "gachix";
    let private_key_path = temp_path.join("cache.secret");
    let public_key_path = temp_path.join("cache.pub");
    let mut child = Command::new("nix-store")
        .arg("--generate-binary-cache-key")
        .arg(&key_name)
        .arg(&private_key_path)
        .arg(&public_key_path)
        .spawn()?;
    let status = child.wait()?;
    assert!(status.success());

    // Add package to Gachix
    let config = HashMap::from([(
        "GACHIX__STORE__SIGN_PRIVATE_KEY_PATH",
        private_key_path.as_os_str().to_str().unwrap(),
    )]);
    common::add_to_cache(&store_path, &repo_path, Some(config))?;

    // Delete the package such that Nix will try to fetch it later
    common::delete_nix_package(package_name)?;

    // Start the server
    let _server = common::CacheServer::start(port, &repo_path)?;

    let public_key = fs::read_to_string(public_key_path)?;
    let output = Command::new("nix")
        .arg("build")
        .arg(format!("{}#{}", NIXPGKS_VERSION, package_name))
        .arg("--no-link")
        .arg("--option")
        .arg("substituters")
        .arg(base_url)
        .arg("--option")
        .arg("trusted-public-keys")
        .arg(public_key)
        .arg("--debug")
        .output()?;
    // TODO: It should test whether the path was actually substituted
    Ok(())
}

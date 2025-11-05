mod common;
use anyhow::{Result, bail};
use bytes::Buf;
use nix_nar::Decoder;
use regex::Regex;
use reqwest::StatusCode;
use tempfile::TempDir;

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
    common::add_to_cache(&store_path, &repo_path)?;
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
    common::add_to_cache(&store_path, &repo_path)?;
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
    common::add_to_cache(&store_path, &repo_path)?;

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

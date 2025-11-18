mod common;
use std::collections::HashMap;

use anyhow::Result;
use tempfile::TempDir;

#[test]
fn test_no_peers_leads_to_error() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let temp_dir_path = temp_dir.path();

    let repo = temp_dir_path.join("cache");
    let package_path = common::build_nix_package("hello")?;
    let config = HashMap::from([("GACHIX__STORE__USE_LOCAL_NIX_DAEMON", "0")]);

    let result = common::add_to_cache(&package_path, &repo, Some(config));
    assert!(result.is_err());

    Ok(())
}

#[test]
fn test_fetch_entire_closure_from_git_remote() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let temp_dir_path = temp_dir.path();

    let holder_path = temp_dir_path.join("holder");
    let requester_path = temp_dir_path.join("requester");

    let package_path = common::build_nix_package("hello")?;

    let config = HashMap::from([
        ("GACHIX__STORE__REMOTES", holder_path.to_str().unwrap()),
        ("GACHIX__STORE__USE_LOCAL_NIX_DAEMON", "0"),
    ]);

    common::add_to_cache(&package_path, &holder_path, None)?;
    common::add_to_cache(&package_path, &requester_path, Some(config))?;

    Ok(())
}

// This test only really tests partial fetches if we assume that it always tries to fetch from git
// remotes first
#[test]
fn test_partial_fetch_from_git_remotes() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let temp_dir_path = temp_dir.path();

    let holder_path = temp_dir_path.join("holder");
    let requester_path = temp_dir_path.join("requester");

    let glibc_path = common::build_nix_package("glibc")?;
    let hello_path = common::build_nix_package("hello")?;

    let config = HashMap::from([
        ("GACHIX__STORE__REMOTES", holder_path.to_str().unwrap()),
        ("GACHIX__STORE__USE_LOCAL_NIX_DAEMON", "1"),
    ]);

    common::add_to_cache(&glibc_path, &holder_path, None)?;
    common::add_to_cache(&hello_path, &requester_path, Some(config))?;

    Ok(())
}

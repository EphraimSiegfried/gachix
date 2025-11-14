mod common;
use std::collections::HashMap;

use anyhow::Result;
use tempfile::TempDir;

#[test]
fn test_root_package_fetch() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let temp_dir_path = temp_dir.path();

    let holder_path = temp_dir_path.join("holder");
    let requester_path = temp_dir_path.join("requester");

    let package_path = common::build_nix_package("hello")?;

    let config = HashMap::from([("GACHIX_STORE_REMOTE", holder_path.to_str().unwrap())]);
    common::add_to_cache(&package_path, &holder_path, Some(config))?;

    common::add_to_cache(&package_path, &requester_path, None)?;

    Ok(())
}

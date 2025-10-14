use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CacheInfo {
    store_dir: String,
    // want_mass_query: bool,
    want_mass_query: u8,
    priority: usize,
}

impl CacheInfo {
    pub fn default() -> Self {
        Self {
            store_dir: "/nix/store".to_string(),
            want_mass_query: 0,
            priority: 50,
        }
    }
}

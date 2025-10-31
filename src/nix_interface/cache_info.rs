use std::fmt::Display;

pub struct CacheInfo {
    store_dir: String,
    want_mass_query: bool,
    priority: usize,
}

impl Display for CacheInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys = ["StoreDir", "WantMassQuery", "Priority"];
        let mass_query = if self.want_mass_query { "1" } else { "0" };
        let values = [&self.store_dir, mass_query, &self.priority.to_string()];
        let output = keys
            .iter()
            .zip(values)
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect::<Vec<String>>()
            .join("\n");

        f.write_str(&output)?;
        Ok(())
    }
}

impl CacheInfo {
    pub fn default() -> Self {
        Self {
            store_dir: "/nix/store".to_string(),
            want_mass_query: false,
            priority: 50,
        }
    }
}

use anyhow::Result;
use std::{collections::HashMap, fmt::Display};

use crate::nix_interface::path::NixPath;

const KEYS: [&str; 10] = [
    "StorePath",
    "URL",
    "Compression",
    "FileHash",
    "FileSize",
    "NarHash",
    "NarSize",
    "References",
    "Deriver",
    "Sig",
];

#[derive(Debug, Clone)]
pub struct NarInfo {
    pub store_path: NixPath,
    pub key: String,
    pub url: Option<String>,
    pub compression_type: Option<String>,
    pub file_hash: String,
    pub file_size: u64,
    pub nar_hash: String,
    pub nar_size: u64,
    pub references: Vec<NixPath>,
    pub deriver: Option<NixPath>,
    pub signature: Option<String>,
}

impl NarInfo {
    pub fn new(
        store_path: NixPath,
        key: String,
        file_hash: String,
        file_size: u64,
        compression_type: Option<String>,
        nar_hash: String,
        nar_size: u64,
        deriver: Option<NixPath>,
        references: Vec<NixPath>,
        signature: Option<String>,
    ) -> Self {
        Self {
            store_path: store_path,
            key: key,
            url: None,
            compression_type: compression_type,
            file_hash: file_hash,
            file_size: file_size,
            nar_hash: nar_hash,
            nar_size: nar_size,
            references: references,
            deriver: deriver,
            signature: signature,
        }
    }

    pub fn parse(content: &str) -> Result<Self> {
        let hashmap: HashMap<&str, &str> = content
            .trim()
            .lines()
            .enumerate()
            .map(|(line_num, line)| {
                line.split_once(": ")
                    .map(|(k, v)| Ok((k.trim(), v.trim())))
                    .unwrap_or_else(|| {
                        Err(anyhow::anyhow!(
                            "invalid narinfo: line {line_num} does not contain 'key: value'. Instead found: '{line}'"
                        ))
                    })
            })
            .collect::<Result<_>>()?;

        let get = |k| {
            hashmap
                .get(k)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("Missing Narinfo key: {k}"))
        };

        let store_path_str = get("StorePath")?;
        let deriver_str = get("Deriver")?;
        let references_str = get("References")?;
        let url_str = get("URL")?;

        let key = url_str
            .split("nar/")
            .last()
            .ok_or_else(|| anyhow::anyhow!("Narinfo does not contain a valid URL"))?
            .split(".")
            .next()
            .ok_or_else(|| anyhow::anyhow!("Narinfo does not contain a valid URL"))?
            .to_string();

        let compression_type = match get("Compression")? {
            "" => None,
            s => Some(s.to_string()),
        };

        let deriver = match deriver_str {
            "" => None,
            s => Some(NixPath::new(s)?),
        };

        let references = match references_str {
            "" => Vec::new(),
            r => r
                .split(' ')
                .map(NixPath::new)
                .collect::<Result<Vec<NixPath>>>()?,
        };

        Ok(Self {
            store_path: NixPath::new(store_path_str)?,
            key,
            url: Some(url_str.to_string()),
            compression_type,
            file_hash: get("FileHash")?.to_string(),
            file_size: get("FileSize")?.parse::<u64>()?,
            nar_hash: get("NarHash")?.to_string(),
            nar_size: get("NarSize")?.parse::<u64>()?,
            references,
            deriver,
            signature: Some(get("Sig")?.to_string()),
        })
    }

    pub fn get_dependencies(&self) -> Vec<&NixPath> {
        self.references
            .iter()
            .filter(|r| **r != self.store_path)
            .collect()
    }
}

impl Display for NarInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file_size_str = self.file_size.to_string();
        let nar_size_str = self.nar_size.to_string();

        let references_str = self
            .references
            .iter()
            .map(|s| format!("{}-{}", s.get_base_32_hash(), s.get_name()))
            .collect::<Vec<_>>()
            .join(" ");

        let deriver = match &self.deriver {
            Some(d) => format!("{}-{}", d.get_base_32_hash(), d.get_name()),
            None => "".to_string(),
        };

        let url = self.url.clone().unwrap_or(format!("nar/{}.nar", self.key));
        let values = [
            self.store_path.get_path(),
            url.as_str(),
            self.compression_type.as_deref().unwrap_or("none"),
            self.file_hash.as_str(),
            file_size_str.as_str(),
            self.nar_hash.as_str(),
            nar_size_str.as_str(),
            references_str.as_str(),
            &deriver,
            self.signature.as_deref().unwrap_or(""),
        ];

        for (key, value) in KEYS.iter().zip(values) {
            write!(f, "{}: {}\n", key, value)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_parse_narinfo() -> Result<()> {
        let content = r#"
StorePath: /nix/store/iylhaki6573cpsvspivjfsim700n46r3-kitty-0.43.1
URL: nar/0lfjpl49j2na01l1zdmyszxz5wr957kl5qxn278alyv0fvxh2lab.nar.xz
Compression: xz
FileHash: sha256:0lfjpl49j2na01l1zdmyszxz5wr957kl5qxn278alyv0fvxh2lab
FileSize: 18391180
NarHash: sha256:163xjwsv9c433ivkycx26g7yb7ig2zq6h1vnmk9faah7qiqb4app
NarSize: 63152768
References: 3m5cgk18mw6lrlbdawc71rlx0sqw6z8i-imagemagick-7.1.2-5 49c4bxmqq5y53y38v7amdcs05d061wvr-tzdata-2025b 5vnba43n1w87cs2i2dd242zy88k4dwf9-zlib-1.3.1 8v0n10sz2rlh6iz2vc95haryx2dvgs1y-harfbuzz-11.2.1 b9crq3qpr3wnma88kwlx4jp1kly45v91-iana-etc-20250505 bsnylm1xz0d3350lzij8yw26wr0qywg0-ncurses-6.5-dev hxmkygn2zl0f2w9kbixmm50lsy60zya0-openssl-3.5.2 iylhaki6573cpsvspivjfsim700n46r3-kitty-0.43.1 j4ik7djz2f6pwavxp2j91615fy9p93j9-lcms2-2.17 k5c8pi2sfycps2ig2z4flh0yr95f79s6-kitty-0.43.1-terminfo n50rq66j0a9dfmw38yd6nwkca9fhb55p-mailcap-2.1.54 ncn2lbkihg40bnihakgxancwsrs39xch-libpng-apng-1.6.50 nrz6nhv4vpa1j0dlyydjxf23gawfl9xy-xxHash-0.8.3 xjpv7j44jn7mifw8r69p7shrsh1aqmnf-python3-3.13.7
Deriver: sm4iyczmq406d83inf5s1ynr5h5h4sym-kitty-0.43.1.drv
Sig: cache.nixos.org-1:NqjenY5yhRXNsUTUHwR9Io9xoD8B2XIUJQJFt6gBl9ik55Rcnj7wdHV1L8YTk4MtO4PEabpfdckXRpVgPh4jDg==
        "#;
        let narinfo = NarInfo::parse(content)?;
        assert_eq!(content.trim(), narinfo.to_string().trim());
        Ok(())
    }
}

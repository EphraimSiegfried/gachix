use crate::nix_interface::path::NixPath;
use anyhow::{Result, anyhow};
use base64::{Engine, prelude::BASE64_STANDARD};
use ring::signature::Ed25519KeyPair;
use std::str::FromStr;

pub const NUM_SEED_BYTES: usize = 32;
pub const NUM_PUBLIC_KEY_BYTES: usize = 32;
pub const NUM_SECRET_KEY_BYTES: usize = NUM_SEED_BYTES + NUM_PUBLIC_KEY_BYTES;

#[derive(Clone)]
pub struct PrivateKey {
    pub name: String,
    seed: [u8; NUM_SEED_BYTES],
    public_key: [u8; NUM_PUBLIC_KEY_BYTES],
}

impl PrivateKey {
    pub fn sign<M: AsRef<[u8]>>(&self, data: M) -> Vec<u8> {
        let key_pair = Ed25519KeyPair::from_seed_and_public_key(&self.seed, &self.public_key)
            .expect("Valid keys stored in struct");

        let sig = key_pair.sign(data.as_ref());
        sig.as_ref().to_vec()
    }
}

impl FromStr for PrivateKey {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.splitn(2, ':');
        let name = split
            .next()
            .ok_or_else(|| anyhow!("Could not retrieve name from private key"))?;
        let key_base64 = split
            .next()
            .ok_or_else(|| anyhow!("Could not retrieve key from private key"))?;
        let key_bytes = BASE64_STANDARD.decode(key_base64)?;
        let seed = key_bytes[0..NUM_SEED_BYTES].try_into()?;
        let public_key = key_bytes[NUM_SEED_BYTES..NUM_SECRET_KEY_BYTES].try_into()?;
        Ok(Self {
            name: name.to_string(),
            seed: seed,
            public_key: public_key,
        })
    }
}

pub fn fingerprint_store_object(
    store_path: &NixPath,
    nar_hash: &str,
    nar_size: u64,
    references: &[NixPath],
) -> String {
    let references_str = references
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "1;{};{};{};{}",
        store_path, nar_hash, nar_size, references_str
    )
}

#[cfg(test)]
mod tests {

    use super::*;
    use ring::signature::{self, UnparsedPublicKey};

    #[test]
    fn test_signature() -> Result<()> {
        let data = "1;/nix/store/02bfycjg1607gpcnsg8l13lc45qa8qj3-libssh2-1.10.0;sha256:1l29f8r5q2739wnq4i7m2v545qx77b3wrdsw9xz2ajiy3hv1al8b;294664;/nix/store/02bfycjg1607gpcnsg8l13lc45qa8qj3-libssh2-1.10.0,/nix/store/1l4r0r4ab3v3a3ppir4jwiah3icalk9d-zlib-1.2.11,/nix/store/gf6j3k1flnhayvpnwnhikkg0s5dxrn1i-openssl-1.1.1l,/nix/store/z56jcx3j1gfyk4sv7g8iaan0ssbdkhz1-glibc-2.33-56";
        let secret_key_str = "cache.example.org-1:ZJui+kG6vPCSRD4+p1P4DyUVlASmp/zsaeN84PTFW28tj2/PtQWvFWK6Mw+ay8kGif8AZkR5KosHLvuwlzDlgg==";
        let secret_key = PrivateKey::from_str(secret_key_str)?;
        let private_key_bytes =
            BASE64_STANDARD.decode("LY9vz7UFrxViujMPmsvJBon/AGZEeSqLBy77sJcw5YI=")?;
        let public_key = UnparsedPublicKey::new(&signature::ED25519, private_key_bytes);

        let signature = secret_key.sign(data);
        assert!(public_key.verify(data.as_bytes(), &signature).is_ok());
        Ok(())
    }

    // #[test]
    // fn test_fingerprint() {
    //     let store_path = "/nix/store/2bcv91i8fahqghn8dmyr791iaycbsjdd-hello-2.12.2";
    //     let nar_hash = "sha256:0rmalafq2v3k7a83jcmh8hnh5kilbbzqf1rkzsdz0fv1nayscp04";
    //     let nar_size = 274568;
    //     let references = vec![
    //         "2bcv91i8fahqghn8dmyr791iaycbsjdd-hello-2.12.2",
    //         "xx7cm72qy2c0643cm1ipngd87aqwkcdp-glibc-2.40-66",
    //     ];
    //
    //     let fingerprint = fingerprint_store_object(store_path, nar_hash, nar_size, references);
    //     let expected = b"1;/nix/store/syd87l2rxw8cbsxmxl853h0r6pdwhwjr-curl-7.82.0-bin;sha256:1b4sb93wp679q4zx9k1ignby1yna3z7c4c2ri3wphylbc2dwsys0;196040;/nix/store/0jqd0rlxzra1rs38rdxl43yh6rxchgc6-curl-7.82.0";
    //     assert_eq!(fingerprint, expected);
    // }
}

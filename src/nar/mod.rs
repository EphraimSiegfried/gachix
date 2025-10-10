use crate::nar;
pub mod decode;
pub mod encode;
pub use nar::encode::NarGitEncoder;

const NIX_VERSION_MAGIC: &[u8] = b"nix-archive-1";
const PAD_LEN: usize = 8;

pub mod repository;
pub use repository::GitRepo;
pub mod store;

const SINGLE_FILE_PACKAGE_MARKER: &str = "gachix-single-file";

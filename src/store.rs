use git2::{FileMode, Repository, Signature, Time, Tree};
use nix_base32::to_nix_base32;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub struct CaCache<'a> {
    repo: Repository,
    sig: Signature<'a>,
}

impl<'a> CaCache<'a> {
    pub fn new(path_to_repo: &str) -> Result<Self, git2::Error> {
        let path_to_repo = Path::new(path_to_repo);
        let repo = if path_to_repo.exists() {
            Repository::open(path_to_repo)?
        } else {
            Repository::init(path_to_repo)?
        };
        let sig = Signature::new("gachix", "gachix@gachix.com", &Time::new(0, 0))?;
        Ok(Self { repo, sig })
    }

    pub fn add(&self, path: &str) -> Result<(String, String), git2::Error> {
        let mut file = File::open(path).expect("Failed to open file");
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).expect("Failed to read file");
        let blob_oid = self.repo.blob(&buffer)?;

        let file_hash = base32_encode(&sha256_hash(&buffer));

        let head = self.repo.head().ok();
        let parent_commit = if let Some(head_ref) = head {
            Some(head_ref.peel_to_commit()?)
        } else {
            None
        };

        let last_tree = parent_commit.as_ref().and_then(|commit| commit.tree().ok());

        let mut tree_builder = self.repo.treebuilder(last_tree.as_ref())?;
        tree_builder.insert(&file_hash, blob_oid, FileMode::Blob.into())?;
        let tree_oid = tree_builder.write()?;
        let tree = self.repo.find_tree(tree_oid)?;

        let parents: Vec<&git2::Commit> = match &parent_commit {
            Some(commit) => vec![commit],
            None => vec![],
        };

        self.repo
            .commit(Some("HEAD"), &self.sig, &self.sig, "", &tree, &parents)?;

        Ok((file_hash, blob_oid.to_string()))
    }

    pub fn query(&self, hash_value: &str) -> Option<String> {
        self.repo.head().ok().and_then(|r| {
            r.peel_to_commit().ok().and_then(|c| {
                c.tree().ok().and_then(|t| {
                    t.get_name(&hash_value).and_then(|entry| {
                        entry.to_object(&self.repo).ok().and_then(|obj| {
                            obj.as_blob()
                                .and_then(|blob| String::from_utf8(blob.content().to_vec()).ok())
                        })
                    })
                })
            })
        })
    }
}

fn sha256_hash(buf: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(buf);
    hasher.finalize().to_vec()
}

fn base32_encode(hash: &[u8]) -> String {
    to_nix_base32(hash)
}

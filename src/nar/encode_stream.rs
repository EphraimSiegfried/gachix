use super::{NIX_VERSION_MAGIC, PAD_LEN};
use anyhow::{Result, anyhow};
use bytes::Bytes;
use futures::Stream;
use git2::{FileMode, ObjectType, Oid, Repository};
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::task::{Context, Poll};
use std::vec::IntoIter;

#[derive(Debug)]
struct OwnedTreeEntry {
    id: Oid,
    filemode: i32,
    name: Vec<u8>,
}

fn write_padded_bytes(bytes: &[u8]) -> Bytes {
    let len = bytes.len() as u64;
    let len_bytes = len.to_le_bytes();

    let remainder = bytes.len() % PAD_LEN;
    let padding_len = if remainder > 0 {
        PAD_LEN - remainder
    } else {
        0
    };

    let total_len = 8 + bytes.len() + padding_len;
    let mut buf = Vec::with_capacity(total_len);

    buf.extend_from_slice(&len_bytes);
    buf.extend_from_slice(bytes);
    if padding_len > 0 {
        buf.extend_from_slice(&[0u8; PAD_LEN][..padding_len]);
    }

    Bytes::from(buf)
}

enum TraversalState {
    StartNode(Oid, i32),
    ProcessTreeEntries(IntoIter<OwnedTreeEntry>),
    FinishTreeEntry,
    FinishNode,
}

pub struct NarGitStream {
    repo: Arc<RwLock<Repository>>,
    stack: Vec<TraversalState>,
    pending_chunks: VecDeque<Result<Bytes>>,
}

impl NarGitStream {
    pub fn new(repo: Arc<RwLock<Repository>>, root_obj: Oid, root_obj_filemode: i32) -> Self {
        let mut pending_chunks = VecDeque::new();
        pending_chunks.push_back(Ok(write_padded_bytes(NIX_VERSION_MAGIC)));

        let stack = vec![
            TraversalState::FinishNode,
            TraversalState::StartNode(root_obj, root_obj_filemode),
        ];

        NarGitStream {
            repo,
            stack,
            pending_chunks,
        }
    }
}

impl Stream for NarGitStream {
    type Item = Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(chunk) = self.pending_chunks.pop_front() {
                return Poll::Ready(Some(chunk));
            }

            let Some(current_state) = self.stack.pop() else {
                return Poll::Ready(None);
            };

            match current_state {
                TraversalState::StartNode(oid, filemode) => {
                    let kind = if filemode == <FileMode as Into<i32>>::into(FileMode::Tree) {
                        ObjectType::Tree
                    } else {
                        ObjectType::Blob
                    };

                    self.pending_chunks.push_back(Ok(write_padded_bytes(b"(")));
                    self.pending_chunks
                        .push_back(Ok(write_padded_bytes(b"type")));

                    enum OwnedData {
                        TreeEntries(IntoIter<OwnedTreeEntry>),
                        Blob { content: Vec<u8>, executable: bool },
                        LinkTarget(Vec<u8>),
                    }

                    let (node_type_str, owned_data) = {
                        let repo = self.repo.read().unwrap();
                        let Ok(obj) = repo.find_object(oid, Some(kind)) else {
                            let err = anyhow!("Could not find object with oid {}", oid);
                            return Poll::Ready(Some(Err(err)));
                        };

                        match kind {
                            ObjectType::Tree => {
                                let tree = obj.as_tree().unwrap();
                                let mut entries: Vec<_> = tree
                                    .iter()
                                    .map(|entry| OwnedTreeEntry {
                                        id: entry.id(),
                                        filemode: entry.filemode(),
                                        name: entry.name_bytes().to_vec(),
                                    })
                                    .collect();
                                entries.sort_by(|x, y| x.name.cmp(&y.name));
                                (
                                    b"directory".as_slice(),
                                    Some(OwnedData::TreeEntries(entries.into_iter())),
                                )
                            }
                            ObjectType::Blob => {
                                let blob = obj.as_blob().unwrap();
                                // **Here is the crucial copy**
                                let content = blob.content().to_vec();

                                if filemode
                                    == <FileMode as Into<i32>>::into(FileMode::BlobExecutable)
                                {
                                    (
                                        b"regular".as_slice(),
                                        Some(OwnedData::Blob {
                                            content,
                                            executable: true,
                                        }),
                                    )
                                } else if filemode == <FileMode as Into<i32>>::into(FileMode::Blob)
                                {
                                    (
                                        b"regular".as_slice(),
                                        Some(OwnedData::Blob {
                                            content,
                                            executable: false,
                                        }),
                                    )
                                } else if filemode == <FileMode as Into<i32>>::into(FileMode::Link)
                                {
                                    (b"symlink".as_slice(), Some(OwnedData::LinkTarget(content)))
                                } else {
                                    let err = anyhow!("Unsupported blob filemode: {}", filemode);
                                    return Poll::Ready(Some(Err(err)));
                                }
                            }
                            _ => {
                                let err = anyhow!("Unrecognized file type");
                                return Poll::Ready(Some(Err(err)));
                            }
                        }
                    };

                    self.pending_chunks
                        .push_back(Ok(write_padded_bytes(node_type_str)));

                    if let Some(data) = owned_data {
                        match data {
                            OwnedData::TreeEntries(entries_iter) => {
                                self.stack
                                    .push(TraversalState::ProcessTreeEntries(entries_iter));
                            }
                            OwnedData::Blob {
                                content,
                                executable,
                            } => {
                                if executable {
                                    self.pending_chunks
                                        .push_back(Ok(write_padded_bytes(b"executable")));
                                    self.pending_chunks.push_back(Ok(write_padded_bytes(b"")));
                                }
                                self.pending_chunks
                                    .push_back(Ok(write_padded_bytes(b"contents")));
                                self.pending_chunks
                                    .push_back(Ok(write_padded_bytes(&content)));
                            }
                            OwnedData::LinkTarget(target) => {
                                self.pending_chunks
                                    .push_back(Ok(write_padded_bytes(b"target")));
                                self.pending_chunks
                                    .push_back(Ok(write_padded_bytes(&target)));
                            }
                        }
                    }
                }

                TraversalState::ProcessTreeEntries(mut entries_iter) => {
                    if let Some(entry) = entries_iter.next() {
                        self.stack
                            .push(TraversalState::ProcessTreeEntries(entries_iter));
                        let name_bytes = &entry.name;

                        self.stack.push(TraversalState::FinishTreeEntry);
                        self.stack.push(TraversalState::FinishNode);
                        self.stack
                            .push(TraversalState::StartNode(entry.id, entry.filemode));

                        self.pending_chunks
                            .push_back(Ok(write_padded_bytes(b"entry")));
                        self.pending_chunks.push_back(Ok(write_padded_bytes(b"(")));
                        self.pending_chunks
                            .push_back(Ok(write_padded_bytes(b"name")));
                        self.pending_chunks
                            .push_back(Ok(write_padded_bytes(name_bytes)));
                        self.pending_chunks
                            .push_back(Ok(write_padded_bytes(b"node")));
                    }
                }

                TraversalState::FinishTreeEntry => {
                    self.pending_chunks.push_back(Ok(write_padded_bytes(b")")));
                }

                TraversalState::FinishNode => {
                    self.pending_chunks.push_back(Ok(write_padded_bytes(b")")));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{StreamExt, executor::block_on};
    use git2::Repository;
    use nix_nar::Encoder;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::sync::{Arc, RwLock};
    use tempfile::TempDir;

    #[test]
    fn test_encode() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let base_path = temp_dir.path();
        let repo = Repository::init(base_path.join("repo"))?;
        let file_content = b"test content";
        let oid = repo.blob(file_content)?;

        let file_name = base_path.join("test_file");
        let mut file = File::create(&file_name)?;
        file.write_all(file_content)?;

        let mut expected_nar = Vec::new();
        let mut encoder = Encoder::new(&file_name)?;
        encoder.read_to_end(&mut expected_nar)?;

        let repo = Arc::new(RwLock::new(repo));
        let nar_stream = NarGitStream::new(repo, oid, FileMode::Blob.into());
        let results: Vec<Result<Bytes>> = block_on(nar_stream.collect());
        let mut actual_nar = Vec::new();
        for chunk in results {
            actual_nar.extend_from_slice(&chunk?);
        }

        assert_eq!(
            actual_nar, expected_nar,
            "The NAR output from the stream did not match the expected bytes."
        );

        Ok(())
    }
}

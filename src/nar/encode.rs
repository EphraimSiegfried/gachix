use super::{NIX_VERSION_MAGIC, PAD_LEN};
use anyhow::Result;
use anyhow::anyhow;
use git2::{FileMode, Object, ObjectType, Repository};
use std::io::{self, Write};

pub struct NarGitEncoder<'a> {
    repo: &'a Repository,
    root_obj: &'a Object<'a>,
    root_obj_filemode: i32,
}

impl<'a> NarGitEncoder<'a> {
    pub fn new(repo: &'a Repository, root_obj: &'a Object, root_obj_filemode: i32) -> Self {
        NarGitEncoder {
            repo,
            root_obj,
            root_obj_filemode,
        }
    }

    pub fn encode(self) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        self.encode_into(&mut buffer)?;
        Ok(buffer)
    }

    pub fn encode_into<W: Write>(&self, mut writer: W) -> Result<()> {
        write_padded(&mut writer, NIX_VERSION_MAGIC)?;
        self._encode_into(&mut writer, self.root_obj, self.root_obj_filemode)?;
        Ok(())
    }

    fn _encode_into<W: Write>(&self, writer: &mut W, obj: &Object, filemode: i32) -> Result<()> {
        write_padded(writer, b"(")?;
        write_padded(writer, b"type")?;
        let kind = obj.kind();

        match kind {
            Some(ObjectType::Tree) => {
                write_padded(writer, b"directory")?;

                let tree = obj.as_tree().unwrap();
                let mut entries: Vec<_> = tree.iter().collect();
                // NAR requires directory entries to be sorted by name
                entries.sort_by(|x, y| x.name().unwrap().cmp(&y.name().unwrap()));

                for entry in entries {
                    let entry_obj = entry
                        .to_object(self.repo)
                        .expect("Couldn't transform entry to object");
                    write_padded(writer, b"entry")?;
                    write_padded(writer, b"(")?;
                    write_padded(writer, b"name")?;
                    write_padded(writer, entry.name().unwrap().as_bytes())?;
                    write_padded(writer, b"node")?;

                    self._encode_into(writer, &entry_obj, entry.filemode())?;

                    write_padded(writer, b")")?;
                }
            }
            Some(ObjectType::Blob) => {
                let blob = obj.as_blob().unwrap();

                if filemode == <FileMode as Into<i32>>::into(FileMode::BlobExecutable) {
                    write_padded(writer, b"regular")?;
                    write_padded(writer, b"executable")?;
                    write_padded(writer, b"")?;
                    write_padded(writer, b"contents")?;
                    write_padded(writer, blob.content())?;
                } else if filemode == <FileMode as Into<i32>>::into(FileMode::Blob) {
                    write_padded(writer, b"regular")?;
                    write_padded(writer, b"contents")?;
                    write_padded(writer, blob.content())?;
                } else if filemode == <FileMode as Into<i32>>::into(FileMode::Link) {
                    write_padded(writer, b"symlink")?;
                    write_padded(writer, b"target")?;
                    write_padded(writer, blob.content())?;
                } else {
                    return Err(anyhow!("Unsupported blob filemode: {}", filemode));
                }
            }
            _ => {
                return Err(anyhow!("Unrecognized file type"));
            }
        }
        write_padded(writer, b")")?;
        Ok(())
    }
}

fn write_padded<W: Write>(writer: &mut W, bytes: &[u8]) -> io::Result<()> {
    let len = bytes.len() as u64;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(bytes)?;

    let remainder = bytes.len() % PAD_LEN;
    if remainder > 0 {
        let padding = PAD_LEN - remainder;
        writer.write_all(&[0u8; PAD_LEN][..padding])?;
    }

    Ok(())
}

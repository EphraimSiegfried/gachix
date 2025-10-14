use super::{NIX_VERSION_MAGIC, PAD_LEN};
use anyhow::Result;
use anyhow::anyhow;
use git2::{FileMode, Oid, Repository};
use std::io::Read;

pub struct NarGitDecoder<'a> {
    repo: &'a Repository,
}

impl<'a> NarGitDecoder<'a> {
    pub fn new(repo: &'a Repository) -> Self {
        Self { repo }
    }

    pub fn parse(&self, mut reader: impl Read) -> Result<(Oid, i32)> {
        self.read_expect(NIX_VERSION_MAGIC, &mut reader)?;
        self.recursive_parse(&mut reader)
    }

    fn recursive_parse(&self, reader: &mut impl Read) -> Result<(Oid, i32)> {
        self.read_expect(b"(", reader)?;
        self.read_expect(b"type", reader)?;

        let file_type = self.read_utf8_padded(reader)?;
        let oid;
        let filemode;

        match file_type.as_str() {
            "regular" => {
                let tag = self.read_utf8_padded(reader)?;
                match tag.as_str() {
                    "executable" => {
                        filemode = FileMode::BlobExecutable.into();
                        self.read_expect(b"", reader)?;
                        self.read_expect(b"contents", reader)?;
                    }
                    "contents" => filemode = FileMode::Blob,
                    _ => {
                        return Err(anyhow!(
                            "Expected 'executable' or 'contents', instead found '{}'",
                            tag
                        ));
                    }
                }
                let data = self.read_bytes_padded(reader)?;
                oid = self.repo.blob(&data)?;
                self.read_expect(b")", reader)?;
            }
            "symlink" => {
                self.read_expect(b"target", reader)?;
                let target = self.read_bytes_padded(reader)?;
                oid = self.repo.blob(&target)?;
                filemode = FileMode::Link;
                self.read_expect(b")", reader)?;
            }
            "directory" => {
                let mut directory_entries = Vec::new();
                loop {
                    match self.read_utf8_padded(reader)?.as_str() {
                        "entry" => {
                            self.read_expect(b"(", reader)?;
                            self.read_expect(b"name", reader)?;
                            let name = self.read_utf8_padded(reader)?;
                            self.read_expect(b"node", reader)?;
                            let (oid, filemode) = self.recursive_parse(reader)?;
                            directory_entries.push((oid, filemode, name));
                            self.read_expect(b")", reader)?;
                        }
                        ")" => break,
                        _ => return Err(anyhow!("Incorrect directory field")),
                    };
                }
                let mut tree_builder = self.repo.treebuilder(None)?;
                for (oid, filemode, name) in directory_entries {
                    tree_builder.insert(name, oid, filemode)?;
                }
                oid = tree_builder.write()?;
                filemode = FileMode::Tree;
            }
            _ => return Err(anyhow!("Unrecognized file type")),
        }
        Ok((oid, filemode.into()))
    }

    fn read_expect(&self, expected: &[u8], reader: &mut impl Read) -> Result<()> {
        let mut len_buffer = [0u8; PAD_LEN];
        reader.read_exact(&mut len_buffer[..])?;
        let actual_len = usize::from_le_bytes(len_buffer);
        if expected.len() != actual_len {
            // let mut data_buffer = vec![0u8; actual_len];
            // reader.read_exact(&mut data_buffer)?;
            return Err(anyhow!(
                "Expected '{}' with length {}, instead found something with length {}",
                String::from_utf8(expected.to_vec()).unwrap(),
                expected.len(),
                // String::from_utf8(data_buffer)?,
                actual_len
            ));
        }

        let mut data_buffer = vec![0u8; actual_len];
        reader.read_exact(&mut data_buffer)?;

        if expected != data_buffer {
            if let Ok(content_str) = String::from_utf8(data_buffer) {
                return Err(anyhow!(
                    "Expected '{}' tag, instead found: '{}'",
                    String::from_utf8(expected.to_vec()).unwrap(),
                    content_str
                ));
            } else {
                return Err(anyhow!(
                    "Expected '{}' tag",
                    String::from_utf8(expected.to_vec()).unwrap(),
                ));
            }
        }

        let remainder = data_buffer.len() % PAD_LEN;
        if remainder > 0 {
            let mut buffer = [0u8; PAD_LEN];
            let padding = &mut buffer[0..PAD_LEN - remainder];
            reader.read_exact(padding)?;
            if !buffer.iter().all(|b| *b == 0) {
                return Err(anyhow!("Bad archive padding"));
            }
        }
        Ok(())
    }

    fn read_utf8_padded(&self, reader: &mut impl Read) -> Result<String> {
        let bytes = self.read_bytes_padded(reader)?;
        Ok(String::from_utf8(bytes)?)
    }

    fn read_bytes_padded(&self, reader: &mut impl Read) -> Result<Vec<u8>> {
        let mut len_buffer = [0u8; PAD_LEN];
        reader.read_exact(&mut len_buffer[..])?;
        let len = u64::from_le_bytes(len_buffer);

        let mut data_buffer = vec![0u8; len as usize];
        reader.read_exact(&mut data_buffer)?;

        let remainder = data_buffer.len() % PAD_LEN;
        if remainder > 0 {
            let mut buffer = [0u8; PAD_LEN];
            let padding = &mut buffer[0..PAD_LEN - remainder];
            reader.read_exact(padding)?;
            if !buffer.iter().all(|b| *b == 0) {
                return Err(anyhow!("Bad archive padding"));
            }
        }

        Ok(data_buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use nix_nar::Encoder;
    use std::fs::{self, File};
    use std::io::{Cursor, Read, Write};
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::Path;
    use tempfile::TempDir;
    #[test]
    // fn test() -> Result<()> {
    //     let temp_dir = TempDir::new()?;
    //     let base_path = temp_dir.path();
    //     let repo = Repository::init(base_path.join("repo"))?;
    //     let decoder = NarGitDecoder::new(&repo);
    //
    //     let nar_content = fs::read(
    //         "/Users/siegi/gachix/out/0d7ms7s1svrslydl7x1cnbmn04zsxsgpm9s7rx68qbwyzc3cwn26.nar",
    //     )?;
    //     let cursor = Cursor::new(nar_content);
    //     let (_, _) = decoder.parse(cursor)?;
    //     Ok(())
    // }
    #[test]
    fn test_decode_regular_file() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let base_path = temp_dir.path();
        let file_content = b"This is foo content";

        let file_name = base_path.join("foo_file");
        let mut file = File::create(&file_name)?;
        file.write_all(file_content)?;

        let mut buf = Vec::new();
        let mut encoder = Encoder::new(&file_name)?;
        encoder.read_to_end(&mut buf)?;

        let repo = Repository::init(base_path.join("repo"))?;
        let decoder = NarGitDecoder::new(&repo);

        let (oid, _) = decoder.parse(Cursor::new(buf))?;

        let blob = repo.find_blob(oid)?;
        let blob_content = blob.content();
        assert_eq!(blob_content, file_content);
        Ok(())
    }

    #[test]
    fn test_decode_directory() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let base_path = temp_dir.path();

        // Create a directory structure
        let dir_path = base_path.join("test_dir");
        fs::create_dir(&dir_path)?;

        // Create files inside the directory
        let file1_path = dir_path.join("file1.txt");
        let mut file1 = File::create(&file1_path)?;
        file1.write_all(b"Content of file1")?;

        let file2_name = "file2.txt";
        let file2_content = b"Content of file2";
        let file2_path = dir_path.join(file2_name);
        let mut file2 = File::create(&file2_path)?;
        file2.write_all(file2_content)?;

        // Create a subdirectory
        let subdir_path = dir_path.join("subdir");
        fs::create_dir(&subdir_path)?;

        let subfile_path = subdir_path.join("subfile.txt");
        let mut subfile = File::create(&subfile_path)?;
        subfile.write_all(b"Subdirectory content")?;

        // Generate NAR archive for the entire directory
        let mut buf = Vec::new();
        let mut encoder = Encoder::new(&dir_path)?;
        encoder.read_to_end(&mut buf)?;

        let repo = Repository::init(base_path.join("repo"))?;
        let decoder = NarGitDecoder::new(&repo);

        let (oid, filemode) = decoder.parse(Cursor::new(buf))?;

        // For directories, we should get a tree object
        let tree = repo.find_tree(oid)?;
        assert!(tree.len() == 3, "Directory should contain three entries");

        let file = tree.get_path(Path::new(file2_name))?;
        assert!(
            file.to_object(&repo)?.into_blob().unwrap().content() == file2_content,
            "File Content should match"
        );
        assert_eq!(
            filemode,
            <git2::FileMode as Into<i32>>::into(FileMode::Tree),
            "File should be marked as directory"
        );

        Ok(())
    }

    #[test]
    fn test_decode_executable_file() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let base_path = temp_dir.path();
        let file_content = b"#!/bin/bash\necho 'Hello World'";

        // Create executable file
        let file_name = base_path.join("executable_script");
        let mut file = File::create(&file_name)?;
        file.write_all(file_content)?;

        // Set executable permissions
        let mut permissions = file.metadata()?.permissions();
        permissions.set_mode(0o755); // rwxr-xr-x
        fs::set_permissions(&file_name, permissions)?;

        let mut buf = Vec::new();
        let mut encoder = Encoder::new(&file_name)?;
        encoder.read_to_end(&mut buf)?;

        let repo = Repository::init(base_path.join("repo"))?;
        let decoder = NarGitDecoder::new(&repo);

        let (oid, filemode) = decoder.parse(Cursor::new(buf))?;

        let blob = repo.find_blob(oid)?;
        let blob_content = blob.content();
        assert_eq!(blob_content, file_content);
        assert_eq!(
            filemode,
            <git2::FileMode as Into<i32>>::into(FileMode::BlobExecutable),
            "File should be marked executable"
        );
        Ok(())
    }

    #[test]
    fn test_decode_symbolic_link() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let base_path = temp_dir.path();

        let target_file = base_path.join("target.txt");
        let mut file = File::create(&target_file)?;
        file.write_all(b"This is the target file content")?;

        let link_path = base_path.join("symlink_to_target");
        symlink("target.txt", &link_path)?;

        let mut buf = Vec::new();
        let mut encoder = Encoder::new(&link_path)?;
        encoder.read_to_end(&mut buf)?;

        let repo = Repository::init(base_path.join("repo"))?;
        let decoder = NarGitDecoder::new(&repo);

        let (oid, filemode) = decoder.parse(Cursor::new(buf))?;

        let blob = repo.find_blob(oid)?;
        let blob_content = blob.content();
        assert_eq!(
            blob_content, b"target.txt",
            "Symlink should store target path"
        );
        assert_eq!(
            filemode,
            <git2::FileMode as Into<i32>>::into(FileMode::Link),
            "File should be marked as link"
        );
        Ok(())
    }
}

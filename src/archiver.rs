use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use std::io::{self, Write};

use crate::index::Index;
use crate::utils::{GenericFile, blake3_hash, compress, encrypt};

fn list_all_files_recursive(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    recurse_dir(dir, dir, &mut files)?;
    Ok(files)
}

fn recurse_dir(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Recurse into subdirectories
            recurse_dir(root, &path, files)?;
        } else if path.is_file() {
            if let Ok(relative_path) = path.strip_prefix(root) {
                files.push(relative_path.to_path_buf());
            }
        } else {
            return Err(anyhow!("Non file object"));
        }
    }
    Ok(())
}

pub(crate) fn build_archive(
    source: &Path,
    archive_file: &mut GenericFile,
    id_file: &Path,
    level: i32,
) -> Result<()> {
    let file_list = list_all_files_recursive(source)?;
    let mut archive = ZipWriter::new(archive_file);

    let recipients =
        age::IdentityFile::from_file(id_file.to_str().ok_or(anyhow!("Invalid path"))?.to_string())?
            .to_recipients()?;
    let reps: Vec<Box<&dyn age::Recipient>> = recipients
        .iter()
        .map(|r| r.as_ref() as &dyn age::Recipient)
        .map(|r| Box::new(r))
        .collect();
    let mut hashes = HashMap::new();
    let mut mapping = HashMap::new();
    for (i, in_path) in file_list.iter().enumerate() {
        println!("Now working on {} of {}", i, file_list.len());
        let mut read_path = PathBuf::new();
        read_path.push(source);
        read_path.push(in_path);
        let raw = fs::read(read_path)?;
        let hash = blake3_hash(&raw);
        let processed = encrypt(&compress(&raw, level)?, &reps)?;
        hashes.insert(i as u64, hash);
        archive.start_file(format!("{i}"), SimpleFileOptions::default())?;
        archive.write_all(&processed)?;
        mapping.insert(in_path.clone(), i as u64);
    }

    let index = Index { mapping, hashes };
    let index_deser = serde_json::to_string(&index)?.as_bytes().to_vec();
    let processed = encrypt(&compress(&index_deser, level)?, &reps)?;
    archive.start_file("zipurat_index_v1", SimpleFileOptions::default())?;
    archive.write_all(&processed)?;

    archive.finish()?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn list_src() {
        let list = list_all_files_recursive(&Path::new(".")).unwrap();
        for l in list {
            println!("{}", l.display());
        }
    }
}

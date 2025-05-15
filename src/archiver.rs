use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use std::io::Write;

use crate::index::Index;
use crate::serializer::{HashedBinRepr, SimpleBinRepr};
use crate::utils::{GenericFile, blake3_hash, compress, encrypt};
use indicatif::{ProgressBar, ProgressStyle};

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
            // return Err(anyhow!("Non file object {}", path.to_string_lossy()));
        }
    }
    Ok(())
}

pub(crate) fn build_archive(
    source: &Path,
    archive: &mut GenericFile,
    recipients: Vec<Box<dyn age::Recipient + Send>>,
    level: i32,
) -> Result<()> {
    let file_list = list_all_files_recursive(source)?;

    let reps: Vec<Box<&dyn age::Recipient>> = recipients
        .iter()
        .map(|r| r.as_ref() as &dyn age::Recipient)
        .map(|r| Box::new(r))
        .collect();
    let mut hashes = HashMap::new();
    let mut dedup_hashes = vec![];
    let mut mapping = HashMap::new();
    let mut sizes = HashMap::new();
    let mut current_index = 0;
    let pb = ProgressBar::new(file_list.len() as u64);
    pb.set_style(ProgressStyle::with_template("{bar:40} {pos:>7}/{len:7}  eta:{eta}").unwrap());

    for (i, in_path) in file_list.iter().enumerate() {
        pb.set_position(i as u64);
        let mut read_path = PathBuf::new();
        read_path.push(source);
        read_path.push(in_path);
        let raw = fs::read(&read_path)?;
        let raw_size = raw.len() as u64;
        let hash = blake3_hash(&raw);
        let processed = encrypt(&compress(&raw, level)?, &reps)?;
        let chunk_len = processed.len() as u64;
        let candidates = dedup_hashes
            .iter()
            .filter(|(_, h)| *h == hash)
            .map(|(p, _)| p);

        let mut dedup_partner = None;
        for c in candidates {
            let mut ref_path = PathBuf::new();
            ref_path.push(source);
            ref_path.push(c);

            if fs::read(ref_path)? == raw && false {
                dedup_partner = Some(c);
                break;
            }
        }

        match dedup_partner {
            None => {
                hashes.insert((current_index as u64, chunk_len), hash.clone());
                sizes.insert((current_index as u64, chunk_len), raw_size);
                archive.write_all(&processed)?;
                mapping.insert(in_path.clone(), (current_index, chunk_len));
                dedup_hashes.push((in_path.clone(), hash.clone()));
                current_index += chunk_len;
            }
            Some(dedup) => {
                let (old_i, old_len) = mapping
                    .get(dedup)
                    .context("Dedup partner not mapped correctly")?;
                mapping.insert(in_path.clone(), (*old_i, *old_len));
            }
        };
    }

    let index = Index {
        mapping,
        hashes,
        sizes,
    };

    // let index_deser = serde_json::to_string(&index)?.as_bytes().to_vec();
    let mut index_deser = vec![];
    index.write_bin(&mut index_deser)?;
    // ciborium::into_writer(&index, &mut index_deser)?;
    let processed = encrypt(&compress(&index_deser, 22)?, &reps)?;
    let index_start = current_index;
    archive.write_all(&processed)?;
    archive.write_all(&index_start.to_le_bytes())?;
    archive.write_all(&(0 as u32).to_le_bytes())?;
    archive.write_all(&(1 as u32).to_le_bytes())?;
    pb.finish_with_message("Archive written");
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

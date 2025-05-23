use anyhow::{Context, Result};
use colored::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use std::io::{Read, Seek};

use crate::index::Index;
use crate::serializer::SimpleBinRepr;
use crate::utils::{GenericFile, blake3_hash_streaming, compress_and_encrypt};
use humansize::{DECIMAL, format_size};
use indicatif::{ProgressBar, ProgressStyle};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha20Rng;

fn list_all_files_recursive(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    recurse_dir_files(dir, dir, &mut files)?;
    Ok(files)
}
fn list_all_empty_dirs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut empties = Vec::new();
    recurse_dir_empties(dir, dir, &mut empties)?;
    Ok(empties)
}

fn recurse_dir_files(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let ls = fs::read_dir(dir)?.collect::<Vec<_>>();
    for entry in ls {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Recurse into subdirectories
            recurse_dir_files(root, &path, files)?;
        } else if path.is_file() {
            if let Ok(relative_path) = path.strip_prefix(root) {
                files.push(relative_path.to_path_buf());
            }
        } else {
            println!(
                "{}:\n{}",
                "Ignoring non-file object".yellow().bold(),
                path.to_string_lossy()
            );
        }
    }

    Ok(())
}
fn recurse_dir_empties(root: &Path, dir: &Path, empties: &mut Vec<PathBuf>) -> Result<()> {
    let ls = fs::read_dir(dir)?.collect::<Vec<_>>();
    for entry in ls {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if fs::read_dir(&path)?.next().is_none() {
                if let Ok(relative_path) = path.strip_prefix(root) {
                    empties.push(relative_path.to_path_buf());
                }
            } else {
                recurse_dir_empties(root, &path, empties)?;
            }
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
    let magic_number = 12219678139600706333_u64;
    magic_number.write_bin(archive)?;
    let mut file_list =
        list_all_files_recursive(source).context("Directory could not be listed")?;
    let mut empty_dirs = list_all_empty_dirs(source).context("Directory could not be listed")?;
    let mut rng = ChaCha20Rng::from_os_rng();

    file_list.shuffle(&mut rng);
    empty_dirs.shuffle(&mut rng);

    let mut hashes = HashMap::new();
    let mut dedup_hashes = vec![];
    let mut mapping = HashMap::new();
    let mut sizes = HashMap::new();
    let mut current_index = 8;
    let pb = ProgressBar::new(file_list.len() as u64);
    pb.set_style(
        ProgressStyle::with_template("{bar:40} {pos:>7}/{len:7}\nfile: {msg}")
            .context("Progress bar error")?,
    );
    println!();

    for (i, in_path) in file_list.iter().enumerate() {
        let mut read_path = PathBuf::new();
        read_path.push(source);
        read_path.push(in_path);
        // let raw = fs::read(&read_path)?;
        // let raw_size = raw.len() as u64;
        let raw_size = fs::metadata(&read_path)?.len();
        pb.set_position(i as u64);
        pb.set_message(format!(
            "{} ({})",
            &in_path.to_string_lossy(),
            format_size(raw_size, DECIMAL)
        ));
        let hash = blake3_hash_streaming(&mut fs::File::open(&read_path)?)?;
        // let processed = encrypt(&compress(&raw, level)?, &recipients)?;
        // let chunk_len = processed.len() as u64;
        let candidates = dedup_hashes
            .iter()
            .filter(|(_, h)| *h == hash)
            .map(|(p, _)| p);

        let mut dedup_partner = None;
        for c in candidates {
            let mut ref_path = PathBuf::new();
            ref_path.push(source);
            ref_path.push(c);

            if files_equal(fs::File::open(&read_path)?, fs::File::open(&ref_path)?)? {
                dedup_partner = Some(c);
                break;
            }
        }

        match dedup_partner {
            None => {
                hashes.insert(current_index, hash);
                sizes.insert(current_index, raw_size);
                let pos_start = archive.stream_position()?;
                compress_and_encrypt(&mut fs::File::open(read_path)?, archive, level, &recipients)?;
                let chunk_len = archive.stream_position()? - pos_start;
                mapping.insert(in_path.clone(), (current_index, chunk_len));
                dedup_hashes.push((in_path.clone(), hash));
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
        magic_number,
        empty_dirs,
    };

    let mut index_deser = vec![];
    index.write_bin(&mut index_deser)?;
    let start_pos = archive.stream_position()?;
    compress_and_encrypt(&mut index_deser.as_slice(), archive, 22, &recipients)?;
    let index_offset = archive.stream_position()? - start_pos;
    index_offset.write_bin(archive)?;
    magic_number.write_bin(archive)?;
    pb.finish_and_clear();
    Ok(())
}

const BUF_SIZE: usize = 8192;

fn files_equal(mut a: impl Read, mut b: impl Read) -> Result<bool> {
    let mut buf_a = [0u8; BUF_SIZE];
    let mut buf_b = [0u8; BUF_SIZE];

    loop {
        let n1 = a.read(&mut buf_a)?;
        let n2 = b.read(&mut buf_b)?;

        if n1 != n2 {
            return Ok(false);
        }
        if n1 == 0 {
            return Ok(true); // both reached EOF
        }
        if buf_a[..n1] != buf_b[..n2] {
            return Ok(false);
        }
    }
}

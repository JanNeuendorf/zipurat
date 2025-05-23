use crate::{
    index::Index,
    utils::{GenericFile, blake3_hash_streaming, decrypt_and_decompress},
};
use anyhow::{Result, anyhow};
use humansize::{DECIMAL, format_size};
use indicatif::{ProgressBar, ProgressStyle};
use std::{
    fs,
    io::{Seek, Write},
    path::Path,
};

pub fn restore_command(
    archive: &mut GenericFile,
    from: &Path,
    to: &Path,
    ids: &Vec<Box<dyn age::Identity>>,
    trust: bool,
) -> Result<()> {
    let index = Index::parse(archive, ids)?;
    if index.is_file(from) {
        copy_file(archive, from, to, &index, ids)
    } else if index.is_dir(from) {
        copy_directory(archive, from, to, &index, ids, trust)
    } else {
        return Err(anyhow!("Path not found"));
    }
}

pub fn stream_file<W: Write>(
    archive: &mut GenericFile,
    from: &Path,
    to: &mut W,
    index: &Index,
    ids: &Vec<Box<dyn age::Identity>>,
) -> Result<()> {
    let (i, len, _) = index.index_length_and_hash(from)?;
    archive.seek(std::io::SeekFrom::Start(i))?;
    decrypt_and_decompress(archive, to, len, ids)?;
    Ok(())
}
pub fn copy_file(
    archive: &mut GenericFile,
    from: &Path,
    to: &Path,
    index: &Index,
    ids: &Vec<Box<dyn age::Identity>>,
) -> Result<()> {
    let mut file = fs::File::create(to)?;
    stream_file(archive, from, &mut file, index, ids)
}

fn copy_directory(
    archive: &mut GenericFile,
    from: &Path,
    to: &Path,
    index: &Index,
    ids: &Vec<Box<dyn age::Identity>>,
    trust: bool,
) -> Result<()> {
    let subindex = index.subindex(from)?;
    let children = subindex.mapping.keys().collect::<Vec<_>>();
    let pb = ProgressBar::new(children.len() as u64);
    pb.set_style(ProgressStyle::with_template("{bar:40} {pos:>7}/{len:7}\nfile: {msg}").unwrap());

    for (i, c) in children.iter().enumerate() {
        let from_path = from.join(c);
        pb.set_position(i as u64);
        let (_, size, hash_ref) = index.index_length_and_hash(&from_path)?;
        pb.set_message(format!(
            "{} ({})",
            &c.to_string_lossy(),
            format_size(size, DECIMAL)
        ));

        let to_path = to.join(c);
        if trust && to_path.exists() {
            let hash_disk = blake3_hash_streaming(&mut fs::File::open(&to_path)?)?;
            if hash_ref == hash_disk {
                continue;
            }
        }
        if let Some(parent) = to_path.parent() {
            fs::create_dir_all(parent)?;
        }
        copy_file(archive, &from_path, &to_path, index, ids)?;
    }
    pb.finish_and_clear();
    let empties = index
        .empty_dirs
        .iter()
        .filter(|p| p.starts_with(from))
        .map(|p| p.strip_prefix(from))
        .collect::<std::result::Result<Vec<_>, _>>()?;

    for e in empties {
        let to_path = to.join(e);
        fs::create_dir_all(to_path)?;
    }
    Ok(())
}

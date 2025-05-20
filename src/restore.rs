use crate::{
    index::Index,
    utils::{GenericFile, blake3_hash},
};
use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use std::{
    fs,
    io::{Read, Write},
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

fn copy_file(
    archive: &mut GenericFile,
    from: &Path,
    to: &Path,
    index: &Index,
    ids: &Vec<Box<dyn age::Identity>>,
) -> Result<()> {
    let mut file = fs::File::create(to)?;
    let content = index.read_file(archive, from, ids)?;
    file.write_all(content.as_slice())?;
    Ok(())
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
    pb.set_style(ProgressStyle::with_template("{bar:40} {pos:>7}/{len:7}  eta:{eta}").unwrap());

    for (i, c) in children.iter().enumerate() {
        pb.set_position(i as u64);

        let from_path = from.join(c);
        let to_path = to.join(c);
        if trust && to_path.exists() {
            let mut buf = vec![];
            fs::File::open(&to_path)?.read_to_end(&mut buf)?;
            let hash_disk = blake3_hash(&buf);
            let (_, _, hash_ref) = index.index_length_and_hash(&from_path)?;
            if hash_ref == hash_disk {
                continue;
            }
        }
        // continue;
        if let Some(parent) = to_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = index.read_file(archive, &from_path, ids)?;
        fs::File::create(&to_path)?.write_all(&content)?;
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

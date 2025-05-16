use crate::{
    index::Index,
    utils::{GenericFile, blake3_hash},
};
use anyhow::{Result, anyhow};
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
    let children = index
        .mapping
        .keys()
        .filter(|p| p.starts_with(from))
        .map(|p| p.strip_prefix(from))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for c in children {
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
        if let Some(parent) = from_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = index.read_file(archive, &from_path, ids)?;
        fs::File::create(&to_path)?.write_all(&content)?;
    }
    Ok(())
}

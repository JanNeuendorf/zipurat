use crate::{index::Index, utils::GenericFile};
use anyhow::{Result, anyhow};
use std::{fs, io::Write, path::Path};

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
    todo!()
}

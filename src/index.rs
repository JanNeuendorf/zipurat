use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use anyhow::Result;
use anyhow::anyhow;

use serde::{Deserialize, Serialize};
use simd_json::prelude::ArrayTrait;
use zstd::zstd_safe::WriteBuf;

use crate::utils::{GenericFile, blake3_hash, decompress, decrypt};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Index {
    pub hashes: HashMap<u64, String>,
    pub mapping: HashMap<PathBuf, (u64, u64)>,
    pub sizes: HashMap<u64, u64>,
}

impl Index {
    pub fn parse(archive: &mut GenericFile, keys: &Vec<Box<dyn age::Identity>>) -> Result<Self> {
        // let len = archive.seek(std::io::SeekFrom::End(0))?;
        let mut u64_buffer = [0_u8; 8];
        let start_prepreads = std::time::Instant::now();

        archive.seek(SeekFrom::End(-16))?;
        archive.read_exact(&mut u64_buffer)?;
        let index_start = u64::from_le_bytes(u64_buffer);
        archive.seek(SeekFrom::Start(index_start))?;
        dbg!(start_prepreads.elapsed());
        let mut buffer = vec![];
        let start_oneread = std::time::Instant::now();
        archive.read_to_end(&mut buffer)?;
        for _b in 0..16 {
            buffer.pop();
        }
        dbg!(start_oneread.elapsed());
        dbg!(&buffer.len());
        let mut content = decrypt(&buffer, keys)?;
        // println!("{}", String::from_utf8(content.clone())?);

        let start = std::time::Instant::now();

        // let deser = simd_json::serde::from_slice(content.as_mut_slice())?;
        let deser = ciborium::from_reader(content.as_slice())?;
        dbg!(start.elapsed());
        Ok(deser)
    }
    pub fn index(&self, path: &Path) -> Option<(u64, u64)> {
        self.mapping.get(path).map(|i| i.clone())
    }
    pub fn index_length_and_hash(&self, path: &Path) -> Result<(u64, u64, String)> {
        let index = self.index(path).ok_or(anyhow!("File not in index"))?;
        let hash = self
            .hashes
            .get(&index.0)
            .ok_or(anyhow!("File hash not found"))?;
        Ok((index.0, index.1, hash.clone()))
    }

    pub fn read_file(
        &self,
        archive: &mut GenericFile,
        path: &Path,
        keys: &Vec<Box<dyn age::Identity>>,
    ) -> Result<Vec<u8>> {
        let (index, len, hash) = self.index_length_and_hash(path)?;
        let content = read_from_raw_index(archive, keys, index, len)?;

        if hash != blake3_hash(&content) {
            return Err(anyhow!("The hash of the file does not match"));
        } else {
            Ok(content)
        }
    }
}
pub fn read_from_raw_index(
    archive: &mut GenericFile,
    keys: &Vec<Box<dyn age::Identity>>,
    index: u64,
    len: u64,
) -> Result<Vec<u8>> {
    let mut buffer = vec![0_u8; len as usize];
    archive.seek(SeekFrom::Start(index))?;
    archive.read_exact(&mut buffer)?;
    let content = decompress(&decrypt(&buffer, keys)?)?;
    Ok(content)
}

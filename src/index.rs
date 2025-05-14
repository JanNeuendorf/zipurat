use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use anyhow::Result;
use anyhow::anyhow;

use serde::{Deserialize, Serialize};

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
        archive.seek(SeekFrom::End(-16))?;
        archive.read_exact(&mut u64_buffer)?;
        let index_start = u64::from_le_bytes(u64_buffer);
        archive.seek(SeekFrom::Start(index_start))?;
        let mut buffer = vec![];
        archive.read_to_end(&mut buffer)?;
        for _b in 0..16 {
            buffer.pop();
        }

        let content = decompress(&decrypt(&buffer, keys)?)?;

        Ok(serde_json::from_str(&String::from_utf8(content)?)?)
    }
    pub fn index(&self, path: &Path) -> Option<(u64, u64)> {
        self.mapping.get(path).map(|i| i.clone())
    }
    pub fn index_length_and_hash(&self, path: &Path) -> Result<(u64, u64, &str)> {
        let index = self.index(path).ok_or(anyhow!("File not in index"))?;
        let hash = self
            .hashes
            .get(&index.0)
            .ok_or(anyhow!("File hash not found"))?;
        Ok((index.0, index.1, hash))
    }

    pub fn read_file(
        &self,
        archive: &mut GenericFile,
        path: &Path,
        keys: &Vec<Box<dyn age::Identity>>,
    ) -> Result<Vec<u8>> {
        let (index, len, hash) = self.index_length_and_hash(path)?;
        let mut buffer = vec![0_u8; len as usize];
        archive.seek(SeekFrom::Start(index))?;
        archive.read_exact(&mut buffer)?;
        let content = decompress(&decrypt(&buffer, keys)?)?;

        if hash != blake3_hash(&content) {
            return Err(anyhow!("The hash of the file does not match"));
        } else {
            Ok(content)
        }
    }
}

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;
use anyhow::anyhow;

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::utils::{GenericFile, blake3_hash, read_decrypted_file_direct};

#[derive(Serialize, Deserialize, Clone)]
pub struct Index {
    hashes: HashMap<u64, String>,
    mapping: HashMap<PathBuf, u64>,
}

impl Index {
    pub fn parse(
        archive: &mut ZipArchive<GenericFile>,
        keys: &Vec<Box<dyn age::Identity>>,
    ) -> Result<Self> {
        let content = read_decrypted_file_direct(archive, "zipurat_index_v1", keys)?;
        Ok(serde_json::from_str(&String::from_utf8(content)?)?)
    }
    pub fn index(&self, path: &Path) -> Option<u64> {
        self.mapping.get(path).map(|i| i.clone())
    }
    pub fn index_and_hash(&self, path: &Path) -> Result<(u64, &str)> {
        let index = self.index(path).ok_or(anyhow!("File not in index"))?;
        let hash = self
            .hashes
            .get(&index)
            .ok_or(anyhow!("File hash not found"))?;
        Ok((index, hash))
    }

    pub fn read_file(
        &self,
        archive: &mut ZipArchive<GenericFile>,
        path: &Path,
        keys: &Vec<Box<dyn age::Identity>>,
    ) -> Result<Vec<u8>> {
        let (index, hash) = self.index_and_hash(path)?;
        let content = read_decrypted_file_direct(archive, &format!("{index}"), keys)?;
        if hash != blake3_hash(&content) {
            return Err(anyhow!("The hash of the file does not match"));
        } else {
            Ok(content)
        }
    }
}

use std::{
    collections::{HashMap, HashSet},
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use anyhow::{Context, Result};

use crate::serializer::SimpleBinRepr;

use crate::utils::{GenericFile, decrypt_and_decompress};

#[derive(Clone, Debug)]
pub struct Index {
    pub hashes: HashMap<u64, [u8; 32]>,
    pub mapping: HashMap<PathBuf, (u64, u64)>,
    pub sizes: HashMap<u64, u64>,
    pub empty_dirs: Vec<PathBuf>,
    pub magic_number: u64,
}

impl Index {
    pub fn parse(archive: &mut GenericFile, keys: &Vec<Box<dyn age::Identity>>) -> Result<Self> {
        archive.seek(SeekFrom::End(-16))?;
        let index_offset = u64::read_bin(archive)?;
        archive.seek(SeekFrom::Current(-(index_offset as i64) - 8))?;
        let mut content = vec![];
        decrypt_and_decompress(archive, &mut content, index_offset, keys)?;

        let deser = Self::read_bin(&mut content.as_slice())?;
        Ok(deser)
    }
    pub fn index(&self, path: &Path) -> Option<(u64, u64)> {
        self.mapping.get(path).copied()
    }
    pub fn index_length_and_hash(&self, path: &Path) -> Result<(u64, u64, [u8; 32])> {
        let index = self.index(path).ok_or(anyhow!("File not in index"))?;
        let hash = self
            .hashes
            .get(&index.0)
            .ok_or(anyhow!("File hash not found"))?;
        Ok((index.0, index.1, *hash))
    }

    pub fn is_file(&self, path: &Path) -> bool {
        self.mapping.contains_key(path)
    }
    #[allow(unused)]
    pub fn is_dir(&self, path: &Path) -> bool {
        if self.is_file(path) {
            return false;
        }
        self.mapping.keys().any(|k| k.starts_with(path))
    }
    pub fn du(&self, path: &Path) -> Result<u64> {
        if self.is_file(path) {
            let mapping = self.mapping.get(path).context("invalid path")?;
            self.sizes
                .get(&mapping.0)
                .context("Size not in index")
                .copied()
        } else {
            let children = self
                .mapping
                .keys()
                .filter(|k| k.starts_with(path))
                .map(|f| self.du(f))
                .collect::<Result<Vec<_>>>()?;
            Ok(children.iter().sum())
        }
    }
    pub fn subindex(&self, subpath: &Path) -> Result<Self> {
        if self.empty_dirs.contains(&subpath.to_path_buf()) {
            return Ok(Self {
                hashes: HashMap::new(),
                mapping: HashMap::new(),
                sizes: HashMap::new(),
                empty_dirs: vec![],
                magic_number: self.magic_number,
            });
        }
        if !self.is_dir(subpath) {
            return Err(anyhow!(
                "{} is not a directory in index",
                subpath.to_string_lossy()
            ));
        }
        let new_mappings = self
            .mapping
            .iter()
            .filter(|(p, _m)| p.starts_with(subpath))
            .map(|(p, m)| (p.strip_prefix(subpath).map(|p| (p, m))))
            .map(|r| r.map(|(k, v)| (k.to_path_buf(), *v)))
            .collect::<std::result::Result<HashMap<_, _>, _>>()?;

        let new_empties = self
            .empty_dirs
            .iter()
            .filter(|p| p.starts_with(subpath))
            .map(|p| (p.strip_prefix(subpath)))
            .map(|r| r.map(|e| e.to_path_buf()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let selected = new_mappings.values().map(|i| i.0).collect::<Vec<_>>();
        let new_hashes = self
            .hashes
            .iter()
            .filter(|(i, _h)| selected.contains(i))
            .map(|(k, v)| (*k, *v))
            .collect::<HashMap<_, _>>();
        let new_sizes = self
            .sizes
            .iter()
            .filter(|(i, _s)| selected.contains(i))
            .map(|(k, v)| (*k, *v))
            .collect::<HashMap<_, _>>();

        Ok(Self {
            hashes: new_hashes,
            mapping: new_mappings,
            sizes: new_sizes,
            empty_dirs: new_empties,
            magic_number: self.magic_number,
        })
    }
    pub fn get_direct_children(&self, path: &Path) -> Result<HashSet<PathBuf>> {
        let mut children = HashSet::new();
        let si = self.subindex(path)?;
        for file in si.mapping.keys() {
            let root = file.components().next().context("No first component")?;
            let childpath = path.to_path_buf().join(root);
            children.insert(childpath);
        }
        for ed in si.empty_dirs {
            let root = ed.components().next().context("No first component")?;
            let childpath = path.to_path_buf().join(root);
            children.insert(childpath);
        }

        Ok(children)
    }

    pub fn search(&self, pattern: &str) -> HashSet<PathBuf> {
        let mut matches = HashSet::new();
        let pattern = pattern.to_lowercase();
        for c in self.mapping.keys().chain(&self.empty_dirs) {
            if let Some(f) = c.file_name().and_then(|f| f.to_str()) {
                if f.to_lowercase().contains(&pattern) {
                    matches.insert(c.to_path_buf());
                }
            }
            if let Some(d) = c
                .parent()
                .and_then(|d| d.file_name())
                .and_then(|d| d.to_str())
            {
                if d.to_lowercase().contains(&pattern) {
                    let parent = c.parent().expect("Must have parent to match").to_path_buf();
                    matches.insert(parent);
                }
            }
        }
        matches
    }
}

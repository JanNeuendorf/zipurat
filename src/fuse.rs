use crate::index::Index;
use crate::restore::stream_file;
use crate::restore::stream_file_head;
use crate::utils::GenericFile;
use anyhow::Context;
use anyhow::Result;
use bimap::BiMap;
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use indexmap::IndexMap;
use libc::ENOENT;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

const TTL: Duration = Duration::from_secs(1); // 1 second
const HEADBYTES: u32 = 50000;

struct ZipuratFS<'a> {
    index: &'a Index,
    archive: &'a mut GenericFile,
    ids: &'a Vec<Box<dyn age::Identity>>,
    ino_table: BiMap<u64, PathBuf>,
    read_cache: FuseCache,
    lookup_cache: HashMap<(u64, String), FileAttr>,
    listing_cache: HashMap<u64, Vec<(u64, FileType, String)>>,
    attribute_cache: HashMap<u64, FileAttr>,
    head_cache: HashMap<u64, Vec<u8>>,
}

impl<'a> ZipuratFS<'a> {
    fn new(
        index: &'a Index,
        archive: &'a mut GenericFile,
        ids: &'a Vec<Box<dyn age::Identity>>,
        max_files: usize,
        max_size: usize,
    ) -> Result<Self> {
        let mut ino_table = BiMap::new();
        ino_table.insert(1, Path::new("").to_path_buf());
        let mut ino: u64 = 2;
        for filepath in index.mapping.keys() {
            ino_table.insert(ino, filepath.clone());
            ino += 1;
            let mut parent_path = filepath.clone();

            while let Some(parent) = parent_path.parent() {
                parent_path = parent.to_path_buf();
                if !ino_table.contains_right(&parent_path) {
                    ino_table.insert(ino, parent_path.clone());
                    ino += 1;
                }
            }
        }
        for emptydirpath in &index.empty_dirs {
            ino_table.insert(ino, emptydirpath.clone());
            ino += 1;
            let mut parent_path = emptydirpath.clone();

            while let Some(parent) = parent_path.parent() {
                parent_path = parent.to_path_buf();
                if !ino_table.contains_right(&parent_path) {
                    ino_table.insert(ino, parent_path.clone());
                    ino += 1;
                }
            }
        }
        Ok(Self {
            index,
            archive,
            ino_table,
            ids,
            read_cache: FuseCache::new(max_size, max_files),
            lookup_cache: HashMap::new(),
            listing_cache: HashMap::new(),
            attribute_cache: HashMap::new(),
            head_cache: HashMap::new(),
        })
    }
    fn get_size_by_ino(&self, ino: u64) -> Result<u64> {
        let path = self.ino_table.get_by_left(&ino).context("Ino not found")?;
        let map_index = self.index.mapping.get(path).context("path not found")?.0;
        self.index
            .sizes
            .get(&map_index)
            .context("Size not found in index")
            .copied()
    }
    fn get_file_attr(&self, path: &Path) -> Result<FileAttr> {
        let map_index = self.index.mapping.get(path).context("path not found")?.0;

        Ok(FileAttr {
            ino: *self
                .ino_table
                .get_by_right(path)
                .context("innode not found")?,
            size: *self.index.sizes.get(&map_index).context("Size not found")?,
            blocks: 1,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind: FileType::RegularFile,
            perm: 0o644,
            nlink: 1,
            uid: 501,
            gid: 20,
            rdev: 0,
            flags: 0,
            blksize: 512,
        })
    }

    fn get_dir_attr(&self, path: &Path) -> Result<FileAttr> {
        let direct_children = self.index.get_direct_children(path)?;
        let num_links = if path.parent().is_some() {
            direct_children.len() + 2
        } else {
            direct_children.len() + 1
        };
        Ok(FileAttr {
            ino: *self
                .ino_table
                .get_by_right(path)
                .context("Innode not found")?,
            size: 0,
            blocks: 0,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind: FileType::Directory,
            perm: 0o755,
            nlink: num_links as u32,
            uid: 501,
            gid: 20,
            rdev: 0,
            flags: 0,
            blksize: 512,
        })
    }
    fn get_general_attr(&self, path: &Path) -> Result<FileAttr> {
        if self.index.is_file(path) {
            self.get_file_attr(path)
        } else {
            self.get_dir_attr(path)
        }
    }
    fn get_parent_inode(&self, path: &Path) -> Option<u64> {
        if path == Path::new("") {
            Some(1)
        } else {
            let p = path.parent()?;
            self.ino_table.get_by_right(p).copied()
        }
    }
}

impl<'a> Filesystem for ZipuratFS<'a> {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if let Some(attr) = self.lookup_cache.get(&(parent, name.display().to_string())) {
            reply.entry(&TTL, attr, 0);
            return;
        }
        let Some(parent_dir) = self.ino_table.get_by_left(&parent) else {
            reply.error(ENOENT);
            return;
        };
        let path = parent_dir.join(name);
        let Ok(attr) = self.get_general_attr(&path) else {
            reply.error(ENOENT);
            return;
        };
        self.lookup_cache
            .insert((parent, name.display().to_string()), attr);
        reply.entry(&TTL, &attr, 0);
    }

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        if let Some(attr) = self.attribute_cache.get(&ino) {
            reply.attr(&TTL, attr);
            return;
        }
        let Some(path) = self.ino_table.get_by_left(&ino) else {
            reply.error(ENOENT);
            return;
        };
        let Ok(attr) = self.get_general_attr(path) else {
            reply.error(ENOENT);
            return;
        };
        self.attribute_cache.insert(ino, attr);
        reply.attr(&TTL, &attr);
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        let Some(path) = self.ino_table.get_by_left(&ino) else {
            reply.error(ENOENT);
            return;
        };
        if !self.index.is_file(path) {
            reply.error(ENOENT);
            return;
        }
        let mut buffer: Vec<u8> = vec![];
        let file_size = self.get_size_by_ino(ino).expect("Could not get file size");
        let read_size = std::cmp::min(size, file_size.saturating_sub(offset as u64) as u32);
        if offset == 0 && size < HEADBYTES {
            if let Some(cached) = self.head_cache.get(&ino) {
                buffer = cached.clone();
            } else {
                println!("loading head {:?}", path);
                if stream_file_head(
                    self.archive,
                    path,
                    &mut buffer,
                    self.index,
                    HEADBYTES as u64,
                    self.ids,
                )
                .is_err()
                {
                    reply.error(ENOENT);
                    return;
                }
                self.head_cache.insert(ino, buffer.clone());
            }

            reply.data(&buffer.as_slice()[offset as usize..offset as usize + read_size as usize]);
            return;
        }

        if let Some(cached) = self.read_cache.get(path) {
            reply.data(&cached[offset as usize..offset as usize + read_size as usize]);
            return;
        } else {
            println!(
                "loading {:?} ({})",
                path,
                humansize::format_size(file_size, humansize::DECIMAL)
            );
            if stream_file(self.archive, path, &mut buffer, self.index, self.ids).is_err() {
                reply.error(ENOENT);
                return;
            }
            self.read_cache.offer(path, buffer.as_slice());
            reply.data(&buffer.as_slice()[offset as usize..offset as usize + read_size as usize]);
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        if let Some(entries) = self.listing_cache.get(&ino).cloned() {
            for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
                if reply.add(entry.0, (i + 1) as i64, entry.1, entry.2) {
                    break;
                }
            }
            reply.ok();
            return;
        }
        let Some(path) = self.ino_table.get_by_left(&ino) else {
            reply.error(ENOENT);
            return;
        };
        if !self.index.is_dir(path) {
            reply.error(ENOENT);
            return;
        }

        let parent_ino = match self.get_parent_inode(path) {
            Some(i) => i,
            _ => ino,
        };
        let mut entries = vec![
            (ino, FileType::Directory, ".".to_string()),
            (parent_ino, FileType::Directory, "..".to_string()),
        ];
        let Ok(children) = self.index.get_direct_children(path) else {
            reply.error(ENOENT);
            return;
        };
        let mut sorted: Vec<PathBuf> = children.iter().map(|p| p.to_owned()).collect();
        sorted.sort();
        for c in &sorted {
            if let Some(i) = self.ino_table.get_by_right(c) {
                let ft = if self.index.is_file(c) {
                    FileType::RegularFile
                } else {
                    FileType::Directory
                };
                let name = c
                    .strip_prefix(path)
                    .expect("File prefix error")
                    .to_str()
                    .expect("must be utf8");
                entries.push((*i, ft, name.to_string()));
            }
        }
        self.listing_cache.insert(ino, entries.clone());

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            if reply.add(entry.0, (i + 1) as i64, entry.1, entry.2) {
                break;
            }
        }
        reply.ok();
    }
}

pub fn mount(
    index: &Index,
    archive: &mut GenericFile,
    mountpoint: &str,
    ids: &Vec<Box<dyn age::Identity>>,
    auto: bool,
    max_files: usize,
    max_size: usize,
) -> Result<()> {
    let mut options = vec![MountOption::RO, MountOption::FSName("zipurat".to_string())];
    if auto {
        options.push(MountOption::AutoUnmount);
    }
    fuser::mount2(
        ZipuratFS::new(index, archive, ids, max_files, max_size)?,
        mountpoint,
        &options,
    )?;
    Ok(())
}

struct FuseCache {
    max_file_size: usize,
    max_file_number: usize,
    content: IndexMap<PathBuf, Vec<u8>>,
}

impl FuseCache {
    fn new(size: usize, number: usize) -> Self {
        Self {
            max_file_size: size,
            max_file_number: number,
            content: IndexMap::new(),
        }
    }

    fn get(&self, path: &Path) -> Option<&[u8]> {
        self.content.get(path).map(|v| v.as_slice())
    }
    fn offer(&mut self, path: &Path, data: &[u8]) {
        if data.len() > self.max_file_size {
            return;
        }
        if self.max_file_number == 0 {
            return;
        }
        while self.content.len() >= self.max_file_number {
            let Some(key) = self.content.keys().next().cloned() else {
                return;
            };
            self.content.shift_remove(&key);
        }
        self.content.insert(path.to_path_buf(), data.to_vec());
    }
}

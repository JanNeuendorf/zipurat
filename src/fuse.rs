use crate::index::Index;
use crate::restore::stream_file;
use crate::utils::GenericFile;
use anyhow::Context;
use anyhow::Result;
use bimap::BiMap;
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use libc::ENOENT;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

const TTL: Duration = Duration::from_secs(1); // 1 second

struct ZipuratFS<'a> {
    index: &'a Index,
    archive: &'a mut GenericFile,
    ids: &'a Vec<Box<dyn age::Identity>>,
    ino_table: BiMap<u64, PathBuf>,
}

impl<'a> ZipuratFS<'a> {
    fn new(
        index: &'a Index,
        archive: &'a mut GenericFile,
        ids: &'a Vec<Box<dyn age::Identity>>,
    ) -> Result<Self> {
        let mut ino_table = BiMap::new();
        let mut ino: u64 = 1;
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
        })
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
        let p = path.parent()?;
        self.ino_table.get_by_right(p).copied()
    }
}

impl<'a> Filesystem for ZipuratFS<'a> {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let Some(parent_dir) = self.ino_table.get_by_left(&parent) else {
            reply.error(ENOENT);
            return;
        };
        let path = parent_dir.join(name);
        let Ok(attr) = self.get_general_attr(&path) else {
            reply.error(ENOENT);
            return;
        };
        reply.entry(&TTL, &attr, 0);
    }

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let Some(path) = self.ino_table.get_by_left(&ino) else {
            reply.error(ENOENT);
            return;
        };
        let Ok(attr) = self.get_general_attr(path) else {
            reply.error(ENOENT);
            return;
        };
        reply.attr(&TTL, &attr);
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        _size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        let Some(path) = self.ino_table.get_by_left(&ino) else {
            reply.error(ENOENT);
            return;
        };
        let mut buffer: Vec<u8> = vec![];
        if stream_file(self.archive, path, &mut buffer, self.index, self.ids).is_ok() {
            reply.data(&buffer.as_slice()[offset as usize..]);
        } else {
            reply.error(ENOENT);
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
            (ino, FileType::Directory, "."),
            (parent_ino, FileType::Directory, ".."),
        ];
        let Ok(children) = self.index.get_direct_children(path) else {
            reply.error(ENOENT);
            return;
        };
        for c in &children {
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
                entries.push((*i, ft, name));
            }
        }

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            // i + 1 means the index of the next entry
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
) -> Result<()> {
    let mut options = vec![MountOption::RO, MountOption::FSName("hello".to_string())];
    options.push(MountOption::AllowRoot);
    options.push(MountOption::AllowOther);
    options.push(MountOption::AutoUnmount);
    fuser::mount2(ZipuratFS::new(index, archive, ids)?, mountpoint, &options)?;
    Ok(())
}

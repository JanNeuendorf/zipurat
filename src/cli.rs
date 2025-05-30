use anyhow::{Context, Result, anyhow};
use colored::*;
use std::{
    fs,
    io::Seek,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use humansize::{DECIMAL, format_size};

use crate::{
    fuse::mount,
    restore::{copy_file, restore_command, stream_file},
    serializer::SimpleBinRepr,
};

#[derive(Parser, Debug)]
#[command(version, about, long_about =Some("Interact with zipurat archives."))]
#[command(propagate_version = true)]
pub struct Cli {
    #[arg(help = "The archive to interact with (can be sftp://...)")]
    archive: String,

    #[arg(long, short, help = "Specific age identity file")]
    identity_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "Create an archive")]
    Create {
        #[arg(short, long, help = "The directory to be archived")]
        source: PathBuf,
        #[arg(short, long, help = "The zstd compression level", default_value = "3")]
        compression_level: i32,
    },
    #[command(about = "Show the contents of a single file", alias = "cat")]
    Show {
        #[arg(help = "The path to the file")]
        path: PathBuf,
        #[arg(short, long, help = "Output file (default stdout)")]
        output: Option<PathBuf>,
    },
    #[command(about = "List a directory", alias = "ls")]
    List {
        #[arg(help = "directory to list")]
        prefix: Option<PathBuf>,
    },
    #[command(about = "Mount an archive with fuse")]
    Mount {
        #[arg(help = "Mount point")]
        mount_point: PathBuf,
    },
    #[command(about = "Search for files or directories", alias = "search")]
    Find {
        #[arg(help = "name to search for")]
        name: String,
    },
    #[command(about = "Restore a file or directory from the archive")]
    Restore {
        #[arg(
            long,
            help = "path to restore, defaults to the whole archive",
            alias = "path"
        )]
        from: Option<PathBuf>,
        #[arg(help = "output")]
        to: PathBuf,
        #[arg(
            short,
            long,
            help = "Do not copy files if hashes already match",
            default_value = "false"
        )]
        trust_hashes: bool,
    },
    #[command(about = "Get the (uncompressed) size")]
    Du {
        #[arg(help = "path")]
        path: Option<PathBuf>,
        #[arg(short, help = "Human readable", default_value = "false")]
        humansize: bool,
    },
    #[command(about = "Get archive information")]
    Info {},
}

use crate::{
    archiver::build_archive,
    index::Index,
    utils::{
        GenericFile, open_local_archive_read, open_local_archive_write, open_remote_archive_read,
        open_remote_archive_write,
    },
};

fn open_general_archive_read(path: &str) -> Result<GenericFile> {
    match parse_sftp_url(path) {
        Ok((host, user, port, path)) => open_remote_archive_read(&host, &user, &path, port),
        Err(_) => open_local_archive_read(path),
    }
}
fn open_general_archive_write(path: &str) -> Result<GenericFile> {
    match parse_sftp_url(path) {
        Ok((host, user, port, path)) => open_remote_archive_write(&host, &user, &path, port),
        Err(_) => open_local_archive_write(path),
    }
}

fn parse_sftp_url(s: &str) -> Result<(String, String, u64, String)> {
    let s = s
        .strip_prefix("sftp://")
        .ok_or(anyhow!("Missing sftp:// scheme"))?;
    let (user_host_port, path) = s.split_once(':').ok_or(anyhow!("Missing ':' after host"))?;
    let (user, host_port) = user_host_port
        .split_once('@')
        .ok_or(anyhow!("Missing user@host"))?;

    let (host, port) = if let Some((h, p)) = host_port.split_once(':') {
        (h, p.parse()?)
    } else {
        (host_port, 22)
    };

    Ok((host.to_string(), user.to_string(), port, path.to_string()))
}

fn load_recipients(path: &str) -> Result<Vec<Box<dyn age::Recipient + Send>>> {
    Ok(age::IdentityFile::from_file(path.to_string())?.to_recipients()?)
}

impl Cli {
    pub fn run(&self) -> Result<()> {
        match &self.command {
            Commands::Create {
                source,
                compression_level,
            } => {
                let recipients = load_recipients(
                    self.identity_file
                        .as_ref()
                        .context("Recipient file must be provided")?
                        .to_str()
                        .context("Path not a valid string")?,
                )?;
                let mut archive = open_general_archive_write(&self.archive)?;
                build_archive(source, &mut archive, recipients, *compression_level)?
            }
            Commands::Show { path, output } => {
                let identities = load_identities(self.identity_file.as_ref())?;
                let mut archive = open_general_archive_read(&self.archive)?;
                show_command(&mut archive, path, identities, output)?
            }
            Commands::List { prefix } => {
                let mut archive = open_general_archive_read(&self.archive)?;
                let identities = load_identities(self.identity_file.as_ref())?;
                let prefix = match prefix {
                    Some(p) => p.clone(),
                    None => PathBuf::new(),
                };

                list_command(&mut archive, &prefix, identities)?
            }
            Commands::Mount { mount_point } => {
                let mut archive = open_general_archive_read(&self.archive)?;
                let identities = load_identities(self.identity_file.as_ref())?;
                let index = Index::parse(&mut archive, &identities)?;

                mount(
                    &index,
                    &mut archive,
                    mount_point.to_str().context("Invalid mount point")?,
                    &identities,
                )?
            }
            Commands::Info {} => {
                let mut archive = open_general_archive_read(&self.archive)?;
                let identities = load_identities(self.identity_file.as_ref())?;
                info_command(&mut archive, identities)?
            }
            Commands::Du { path, humansize } => {
                let mut archive = open_general_archive_read(&self.archive)?;
                let identities = load_identities(self.identity_file.as_ref())?;
                du_command(
                    &mut archive,
                    path.as_ref().unwrap_or(&PathBuf::new()),
                    identities,
                    *humansize,
                )?
            }
            Commands::Restore {
                from,
                to,
                trust_hashes,
            } => {
                let mut archive = open_general_archive_read(&self.archive)?;
                let identities = load_identities(self.identity_file.as_ref())?;
                let from = match from {
                    Some(p) => p.clone(),
                    None => PathBuf::new(),
                };
                restore_command(&mut archive, &from, to, &identities, *trust_hashes)?
            }
            Commands::Find { name: pattern } => {
                let mut archive = open_general_archive_read(&self.archive)?;
                let identities = load_identities(self.identity_file.as_ref())?;
                find_command(&mut archive, pattern, identities)?;
            }
        };

        Ok(())
    }
}
fn show_command(
    archive: &mut GenericFile,
    path: &Path,

    ids: Vec<Box<dyn age::Identity>>,
    out: &Option<PathBuf>,
) -> Result<()> {
    let index = Index::parse(archive, &ids)?;
    match out {
        Some(file) => {
            copy_file(archive, path, file, &index, &ids)?;
        }
        None => {
            let mut stdout = std::io::stdout();
            stream_file(archive, path, &mut stdout, &index, &ids)?;
        }
    }
    Ok(())
}
fn du_command(
    archive: &mut GenericFile,
    path: &Path,
    ids: Vec<Box<dyn age::Identity>>,
    hflag: bool,
) -> Result<()> {
    let index = Index::parse(archive, &ids)?;
    let size = index.du(path)?;
    if hflag {
        println!("{}", format_size(size, DECIMAL))
    } else {
        println!("{size}");
    }
    Ok(())
}

fn list_command(
    archive: &mut GenericFile,
    prefix: &Path,
    ids: Vec<Box<dyn age::Identity>>,
) -> Result<()> {
    let index = Index::parse(archive, &ids)?.subindex(prefix)?;
    let mut children = vec![];
    for path in index.mapping.keys() {
        let first = path
            .components()
            .next()
            .context("Empty entry! (It might be a file and not a directory)")?;
        if !children.contains(&first) {
            children.push(first);
        }
    }
    for p in children {
        if index.is_file(&PathBuf::new().join(p)) {
            let size = index.du(&PathBuf::new().join(p))?;
            let size_fmt = format_size(size, DECIMAL);
            println!("{:12} {}", size_fmt, p.as_os_str().to_string_lossy());
        } else {
            println!(
                "{:12} {}",
                "-".blue().bold(),
                p.as_os_str().to_string_lossy().blue().bold()
            );
        }
    }
    Ok(())
}
fn find_command(
    archive: &mut GenericFile,
    pattern: &str,
    ids: Vec<Box<dyn age::Identity>>,
) -> Result<()> {
    let index = Index::parse(archive, &ids)?;
    let matches = index.search(pattern);
    for p in matches {
        if index.is_file(&p) {
            let size = index.du(&p)?;
            let size_fmt = format_size(size, DECIMAL);
            println!("{:12} {}", size_fmt, p.to_string_lossy());
        } else {
            println!(
                "{:12} {}",
                "-".blue().bold(),
                p.to_string_lossy().blue().bold()
            );
        }
    }
    Ok(())
}
fn info_command(archive: &mut GenericFile, ids: Vec<Box<dyn age::Identity>>) -> Result<()> {
    archive.seek(std::io::SeekFrom::End(-16))?;
    let index_size = u64::read_bin(archive)?;
    let magic_number = u64::read_bin(archive)?;

    let index = Index::parse(archive, &ids)?;
    let mut total_size = 0_u64;
    for k in index.mapping.values() {
        total_size += index.sizes.get(&k.0).context("Size could not be read")?;
    }
    let duplicats = index.mapping.len() - index.hashes.len();
    let compressed_size = archive.seek(std::io::SeekFrom::End(0))?;
    println!("magic number: {:X}", magic_number);
    println!("files: {}", index.mapping.len());
    println!("size original: {}", format_size(total_size, DECIMAL));
    println!("size compressed: {}", format_size(compressed_size, DECIMAL));
    println!(
        "compression ratio: {:.2}",
        (total_size as f64) / (compressed_size as f64)
    );
    println!("duplicate files: {}", duplicats);
    println!("empty directories: {}", index.empty_dirs.len());
    println!("size index: {}", format_size(index_size, DECIMAL));
    Ok(())
}

fn load_identities(provided: Option<&PathBuf>) -> Result<Vec<Box<dyn age::Identity>>> {
    if let Some(file) = provided {
        let ids = age::IdentityFile::from_file(
            file.to_str().context("Invalid path for IDs")?.to_string(),
        )
        .context("Indentity file could not be loaded")?
        .into_identities()?;
        return Ok(ids);
    }
    let mut all_ids = vec![];
    let dir = dirs::config_dir()
        .map(|cfg| cfg.join("age"))
        .context("Home directory not found")?;
    let entries: Vec<_> = fs::read_dir(&dir)
        .context(format!("{} not found", dir.to_string_lossy()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    for f in entries {
        let idf = age::IdentityFile::from_file(
            f.to_str()
                .context("Non utf8 file in zipurat dir")?
                .to_string(),
        );
        if let Ok(idf) = idf {
            if let Ok(mut ids) = idf.into_identities() {
                all_ids.append(&mut ids);
            }
        }
    }
    if all_ids.is_empty() {
        return Err(anyhow!(
            "No valid age IDs found in {}",
            dir.to_string_lossy()
        ));
    }
    Ok(all_ids)
}

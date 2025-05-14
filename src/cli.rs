use anyhow::{Context, Result, anyhow};
use std::{
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use humansize::{DECIMAL, format_size};

#[derive(Parser, Debug)]
#[command(version, about, long_about =Some("Interact with zipurat archives."))]
#[command(propagate_version = true)]
pub struct Cli {
    #[arg(help = "The archive to interact with (can be sftp)")]
    archive: String,

    #[arg(long, short, help = "The age identity file")]
    identity_file: PathBuf,

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
    #[command(about = "Load the contents of a single file", alias = "cat")]
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
    #[command(about = "Get archive information")]
    Info {},
}

use url::Url;

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
    let url = Url::parse(s)?;

    if url.scheme() != "sftp" {
        return Err(anyhow!("URL scheme must be 'sftp'"));
    }

    let host = url.host_str().ok_or(anyhow!("Missing host"))?.to_string();
    let user = url.username();
    if user.is_empty() {
        return Err(anyhow!("User required"));
    }
    let port = url.port().unwrap_or(22);
    let path = url.path().to_string();

    Ok((host, user.to_string(), port.into(), path))
}

fn load_recipients(path: &str) -> Result<Vec<Box<dyn age::Recipient + Send>>> {
    Ok(age::IdentityFile::from_file(path.to_string())?.to_recipients()?)
}
fn load_identities(path: &str) -> Result<Vec<Box<dyn age::Identity>>> {
    Ok(age::IdentityFile::from_file(path.to_string())?.into_identities()?)
}

impl Cli {
    pub fn run(&self) -> Result<()> {
        let recipients = load_recipients(
            self.identity_file
                .to_str()
                .context("Path not a valid string")?,
        )?;
        let identities = load_identities(
            self.identity_file
                .to_str()
                .context("Path not a valid string")?,
        )?;
        match &self.command {
            Commands::Create {
                source,
                compression_level,
            } => {
                let mut archive = open_general_archive_write(&self.archive)?;
                build_archive(source, &mut archive, recipients, *compression_level)?
            }
            Commands::Show { path, output } => {
                let mut archive = open_general_archive_read(&self.archive)?;
                show_command(&mut archive, path, identities, output)?
            }
            Commands::List { prefix } => {
                let mut archive = open_general_archive_read(&self.archive)?;
                let prefix = match prefix {
                    Some(p) => p.clone(),
                    None => PathBuf::new(),
                };

                list_command(&mut archive, &prefix, identities)?
            }
            Commands::Info {} => {
                let mut archive = open_general_archive_read(&self.archive)?;
                info_command(&mut archive, identities)?
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
    let content = index.read_file(archive, path, &ids)?;
    match out {
        Some(file) => {
            std::fs::write(file, content)?;
        }
        None => {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(&content)?;
            handle.flush()?;
        }
    }
    Ok(())
}

fn list_command(
    archive: &mut GenericFile,
    prefix: &Path,
    ids: Vec<Box<dyn age::Identity>>,
) -> Result<()> {
    let index = Index::parse(archive, &ids)?;
    let any = index
        .mapping
        .keys()
        .filter(|p| p.starts_with(prefix))
        .map(|p| p.strip_prefix(prefix))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if any.len() == 0 {
        return Err(anyhow!("directory not found"));
    }
    let mut children = vec![];
    for path in any {
        let first = path
            .components()
            .into_iter()
            .next()
            .context("Empty entry! (It might be a file and not a directory)")?;
        if !children.contains(&first) {
            children.push(first);
        }
    }
    for p in children {
        println!("{}", p.as_os_str().to_string_lossy());
    }
    Ok(())
}
fn info_command(archive: &mut GenericFile, ids: Vec<Box<dyn age::Identity>>) -> Result<()> {
    archive.seek(std::io::SeekFrom::End(-8))?;
    let mut u32_buffer = [0_u8; 4];
    archive.read_exact(&mut u32_buffer)?;
    let major_version = u32::from_le_bytes(u32_buffer);
    archive.read_exact(&mut u32_buffer)?;
    let minor_version = u32::from_le_bytes(u32_buffer);

    let index = Index::parse(archive, &ids)?;
    let mut total_size = 0 as u64;
    for (k, _) in index.mapping.values() {
        total_size += index.sizes.get(&k).unwrap();
    }
    let duplicats = index.mapping.len() - index.hashes.len();
    let compressed_size = archive.seek(std::io::SeekFrom::End(0))?;
    println!("format version: {}.{}", major_version, minor_version);
    println!("files: {}", index.mapping.len());
    println!("size original: {}", format_size(total_size, DECIMAL));
    println!("size compressed: {}", format_size(compressed_size, DECIMAL));
    println!(
        "compression ratio: {:.2}",
        (total_size as f64) / (compressed_size as f64)
    );
    println!("duplicate files: {}", duplicats);
    Ok(())
}

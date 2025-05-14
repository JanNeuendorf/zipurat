use anyhow::{Context, Result, anyhow};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use zip::{ZipArchive, ZipWriter};
use zstd::zstd_safe::WriteBuf;

use clap::{Parser, Subcommand};

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
    #[command(about = "Load the contents of a single file")]
    Show {
        #[arg(help = "The path to the file")]
        path: PathBuf,
        #[arg(short, long, help = "Output file (default stdout)")]
        output: Option<PathBuf>,
    },
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

fn open_general_archive_read(path: &str) -> Result<ZipArchive<GenericFile>> {
    match parse_sftp_url(path) {
        Ok((host, user, port, path)) => open_remote_archive_read(&host, &user, &path, port),
        Err(_) => open_local_archive_read(path),
    }
}
fn open_general_archive_write(path: &str) -> Result<ZipWriter<GenericFile>> {
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
                let archive = open_general_archive_write(&self.archive)?;
                build_archive(source, archive, recipients, *compression_level)?
            }
            Commands::Show { path, output } => {
                let mut archive = open_general_archive_read(&self.archive)?;
                show_command(&mut archive, path, identities, output)?
            }
        };

        Ok(())
    }
}
fn show_command(
    archive: &mut ZipArchive<GenericFile>,
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

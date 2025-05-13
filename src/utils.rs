use anyhow::{Context, Result};
use std::{
    fs,
    io::{Cursor, Read, Seek},
    iter,
    net::TcpStream,
    path::Path,
};
use zip::{ZipArchive, read::ZipFile};

fn read_raw_file_direct<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>> {
    let mut zip_file = archive.by_name(name)?;
    let mut content = vec![];
    zip_file.read_to_end(&mut content)?;
    Ok(content)
}

fn read_decompressed_file_direct<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>> {
    let raw_content = read_raw_file_direct(archive, name)?;
    let mut decompressed = vec![];
    zstd::stream::copy_decode(Cursor::new(raw_content), &mut decompressed)?;
    Ok(decompressed)
}

pub fn read_decrypted_file_direct<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    keys: &Vec<Box<dyn age::Identity>>,
) -> Result<Vec<u8>> {
    let encrypted = read_decompressed_file_direct(archive, name)?;
    let mut decrypted = vec![];
    let decryptor = age::Decryptor::new(&encrypted[..])?;
    let mut reader = decryptor.decrypt(keys.iter().map(|b| b.as_ref()))?;
    reader.read_to_end(&mut decrypted)?;
    Ok(decrypted)
}

fn open_local_archive(filename: &str) -> Result<ZipArchive<GenericFile>> {
    let file = GenericFile::Local(std::fs::File::open(filename)?);
    return Ok(ZipArchive::new(file)?);
}

fn open_remote_archive(
    host: &str,
    user: &str,
    filename: &str,
    port: u64,
) -> Result<ZipArchive<GenericFile>> {
    let tcp = TcpStream::connect(format!("{}:{}", host, port)).unwrap();
    let mut sess = ssh2::Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().unwrap();
    sess.userauth_agent(user).unwrap();
    let sftp = sess.sftp()?;
    let remote_file = sftp.open(filename)?;

    return Ok(ZipArchive::new(GenericFile::Remote(remote_file))?);
}

pub enum GenericFile {
    Local(std::fs::File),
    Remote(ssh2::File),
}

impl Read for GenericFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            GenericFile::Remote(f) => f.read(buf),
            GenericFile::Local(f) => f.read(buf),
        }
    }
}

impl Seek for GenericFile {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match self {
            GenericFile::Remote(f) => f.seek(pos),
            GenericFile::Local(f) => f.seek(pos),
        }
    }
}

pub fn blake3_hash(data: &Vec<u8>) -> String {
    let hash = blake3::hash(&data);
    hash.to_hex().to_string()
}

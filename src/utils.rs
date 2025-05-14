use anyhow::{Context, Result};
use std::{
    fs,
    io::{Cursor, Read, Seek, Write},
    iter,
    net::TcpStream,
    path::Path,
};
use zip::{ZipArchive, ZipWriter, read::ZipFile};

fn read_raw_file_direct<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>> {
    let mut zip_file = archive.by_name(name)?;
    let mut content = vec![];
    zip_file.read_to_end(&mut content)?;
    Ok(content)
}

pub fn read_decompressed_file_direct<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    keys: &Vec<Box<dyn age::Identity>>,
) -> Result<Vec<u8>> {
    let raw_content = read_decrypted_file_direct(archive, name, keys)?;
    let mut decompressed = vec![];
    zstd::stream::copy_decode(Cursor::new(raw_content), &mut decompressed)?;
    Ok(decompressed)
}

fn read_decrypted_file_direct<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    keys: &Vec<Box<dyn age::Identity>>,
) -> Result<Vec<u8>> {
    let encrypted = read_raw_file_direct(archive, name)?;
    let mut decrypted = vec![];
    let decryptor = age::Decryptor::new(&encrypted[..])?;
    let mut reader = decryptor.decrypt(keys.iter().map(|b| b.as_ref()))?;
    reader.read_to_end(&mut decrypted)?;
    Ok(decrypted)
}

pub fn compress(input: &Vec<u8>, level: i32) -> Result<Vec<u8>> {
    let mut out = vec![];
    zstd::stream::copy_encode(input.as_slice(), &mut out, level)?;
    Ok(out)
}

pub fn encrypt(input: &Vec<u8>, keys: &Vec<Box<&dyn age::Recipient>>) -> Result<Vec<u8>> {
    let encryptor = age::Encryptor::with_recipients(keys.iter().map(|k| *k.as_ref()))?;
    let mut encrypted = vec![];
    let mut writer = encryptor.wrap_output(&mut encrypted)?;
    writer.write_all(input)?;
    writer.finish()?;

    Ok(encrypted)
}

pub fn open_local_archive_read(filename: &str) -> Result<ZipArchive<GenericFile>> {
    let f = std::fs::File::open(filename)?;
    let file = GenericFile::Local(f);
    return Ok(ZipArchive::new(file)?);
}
pub fn open_local_archive_write(filename: &str) -> Result<ZipWriter<GenericFile>> {
    let f = std::fs::File::create_new(filename)?;
    let file = GenericFile::Local(f);
    return Ok(ZipWriter::new(file));
}

pub fn open_remote_archive_read(
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
    let remote_file = sftp.open(Path::new(filename))?;

    return Ok(ZipArchive::new(GenericFile::Remote(remote_file))?);
}

pub fn open_remote_archive_write(
    host: &str,
    user: &str,
    filename: &str,
    port: u64,
) -> Result<ZipWriter<GenericFile>> {
    let tcp = TcpStream::connect(format!("{}:{}", host, port)).unwrap();
    let mut sess = ssh2::Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().unwrap();
    sess.userauth_agent(user).unwrap();
    let sftp = sess.sftp()?;
    let remote_file = sftp.create(Path::new(filename))?;

    return Ok(ZipWriter::new(GenericFile::Remote(remote_file)));
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
impl Write for GenericFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            GenericFile::Remote(f) => f.write(buf),
            GenericFile::Local(f) => f.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            GenericFile::Remote(f) => f.flush(),
            GenericFile::Local(f) => f.flush(),
        }
    }
}

pub fn blake3_hash(data: &Vec<u8>) -> String {
    let hash = blake3::hash(&data);
    hash.to_hex().to_string()
}

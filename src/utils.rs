use anyhow::Result;
use std::{
    io::{Read, Seek, Write},
    net::TcpStream,
    path::Path,
};
use zstd::bulk::Compressor;

pub fn compress(input: &Vec<u8>, level: i32) -> Result<Vec<u8>> {
    let mut compressor = Compressor::new(level)?;
    compressor.multithread(num_cpus::get() as u32)?;
    Ok(compressor.compress(input.as_slice())?)
}

pub fn encrypt(input: &Vec<u8>, keys: &Vec<Box<&dyn age::Recipient>>) -> Result<Vec<u8>> {
    let encryptor = age::Encryptor::with_recipients(keys.iter().map(|k| *k.as_ref()))?;
    let mut encrypted = vec![];
    let mut writer = encryptor.wrap_output(&mut encrypted)?;
    writer.write_all(input)?;
    writer.finish()?;

    Ok(encrypted)
}

pub fn decompress(input: &Vec<u8>) -> Result<Vec<u8>> {
    let mut out = vec![];
    zstd::stream::copy_decode(input.as_slice(), &mut out)?;
    Ok(out)
}

pub fn decrypt(input: &Vec<u8>, keys: &Vec<Box<dyn age::Identity>>) -> Result<Vec<u8>> {
    let decryptor = age::Decryptor::new(input.as_slice())?;

    let mut reader = decryptor.decrypt(keys.iter().map(|k| k.as_ref() as &dyn age::Identity))?;
    let mut decrypted = vec![];
    reader.read_to_end(&mut decrypted)?;
    Ok(decrypted)
}

pub fn open_local_archive_read(filename: &str) -> Result<GenericFile> {
    let f = std::fs::File::open(filename)?;
    let file = GenericFile::Local(f);
    return Ok(file);
}
pub fn open_local_archive_write(filename: &str) -> Result<GenericFile> {
    let f = std::fs::File::create_new(filename)?;
    let file = GenericFile::Local(f);
    return Ok(file);
}

pub fn open_remote_archive_read(
    host: &str,
    user: &str,
    filename: &str,
    port: u64,
) -> Result<GenericFile> {
    let tcp = TcpStream::connect(format!("{}:{}", host, port)).unwrap();
    let mut sess = ssh2::Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().unwrap();
    sess.userauth_agent(user).unwrap();
    let sftp = sess.sftp()?;
    let remote_file = sftp.open(Path::new(filename))?;

    return Ok(GenericFile::Remote(remote_file));
}

pub fn open_remote_archive_write(
    host: &str,
    user: &str,
    filename: &str,
    port: u64,
) -> Result<GenericFile> {
    let tcp = TcpStream::connect(format!("{}:{}", host, port)).unwrap();
    let mut sess = ssh2::Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().unwrap();
    sess.userauth_agent(user).unwrap();
    let sftp = sess.sftp()?;
    let remote_file = sftp.create(Path::new(filename))?;

    return Ok(GenericFile::Remote(remote_file));
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
    // *hash.as_bytes()
}

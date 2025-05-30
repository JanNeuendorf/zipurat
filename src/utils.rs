use anyhow::{Result, anyhow};
use std::{
    io::{Read, Seek, Write},
    net::TcpStream,
    path::Path,
};
use zstd::stream::read::{Decoder, Encoder};

pub fn decrypt_and_decompress<R: Read, W: Write>(
    source: &mut R,
    sink: &mut W,
    len: u64,
    ids: &Vec<Box<dyn age::Identity>>,
) -> Result<()> {
    let decryptor = age::Decryptor::new(source.take(len))?;
    let mut decrypted_reader =
        decryptor.decrypt(ids.iter().map(|k| k.as_ref() as &dyn age::Identity))?;
    let mut decoder = Decoder::new(&mut decrypted_reader)?;
    std::io::copy(&mut decoder, sink)?;
    Ok(())
}
pub fn decrypt_and_decompress_head<R: Read, W: Write>(
    source: &mut R,
    sink: &mut W,
    len: u64,
    write_only: u64,
    ids: &Vec<Box<dyn age::Identity>>,
) -> Result<()> {
    let decryptor = age::Decryptor::new(source.take(len))?;
    let mut decrypted_reader =
        decryptor.decrypt(ids.iter().map(|k| k.as_ref() as &dyn age::Identity))?;
    let decoder = Decoder::new(&mut decrypted_reader)?;
    std::io::copy(&mut decoder.take(write_only), sink)?;
    Ok(())
}

pub fn compress_and_encrypt<R: Read, W: Write>(
    source: &mut R,
    sink: &mut W,
    level: i32,
    recipients: &Vec<Box<dyn age::Recipient + Send>>,
) -> Result<()> {
    let reps: Vec<Box<&dyn age::Recipient>> = recipients
        .iter()
        .map(|r| r.as_ref() as &dyn age::Recipient)
        .map(Box::new)
        .collect();

    let mut compressor = Encoder::new(source, level)?;

    let encryptor = age::Encryptor::with_recipients(reps.iter().map(|k| *k.as_ref()))?;
    let mut encrypted_writer = encryptor.wrap_output(sink)?;
    std::io::copy(&mut compressor, &mut encrypted_writer)?;
    encrypted_writer.finish()?;

    Ok(())
}

pub fn open_local_archive_read(filename: &str) -> Result<GenericFile> {
    let f = std::fs::File::open(filename)?;
    let file = GenericFile::Local(f);
    Ok(file)
}
pub fn open_local_archive_write(filename: &str) -> Result<GenericFile> {
    let f = std::fs::File::create_new(filename)?;
    let file = GenericFile::Local(f);
    Ok(file)
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
    let path = Path::new(filename);
    let path = if path.is_absolute() {
        path
    } else {
        &sftp.realpath(Path::new("."))?.join(path)
    };
    let remote_file = sftp.open(path)?;

    Ok(GenericFile::Remote(remote_file))
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
    let path = Path::new(filename);
    let path = if path.is_absolute() {
        path
    } else {
        &sftp.realpath(Path::new("."))?.join(path)
    };
    if sftp.open(path).is_ok() {
        return Err(anyhow!("Archive already exists"));
    }
    let remote_file = sftp.create(path)?;

    Ok(GenericFile::Remote(remote_file))
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

pub fn blake3_hash_streaming<R: Read>(source: &mut R) -> Result<[u8; 32]> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(source)?;
    Ok(*hasher.finalize().as_bytes())
}

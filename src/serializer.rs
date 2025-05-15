use anyhow::{Context, Result};
use simd_json::prelude::ArrayTrait;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    str::FromStr,
};
use zstd::zstd_safe::WriteBuf;

pub trait SimpleBinRepr: Sized {
    fn read_bin<R: Read>(reader: &mut R) -> Result<Self>;
    fn write_bin<W: Write>(&self, writer: &mut W) -> Result<()>;
}

impl SimpleBinRepr for u64 {
    fn read_bin<R: Read>(reader: &mut R) -> Result<Self> {
        let bytes = read_bytes_const::<R, 8>(reader)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn write_bin<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write(&self.to_le_bytes())?;
        Ok(())
    }
}

impl<const N: usize> SimpleBinRepr for [u8; N] {
    fn read_bin<R: Read>(reader: &mut R) -> Result<Self> {
        read_bytes_const::<R, N>(reader)
    }

    fn write_bin<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write(self)?;
        Ok(())
    }
}

impl SimpleBinRepr for String {
    fn read_bin<R: Read>(reader: &mut R) -> Result<Self> {
        let len = u64::read_bin(reader)? as usize;
        let bytes = read_bytes(reader, len)?;
        let string = String::from_utf8(bytes)?;
        Ok(string)
    }

    fn write_bin<W: Write>(&self, writer: &mut W) -> Result<()> {
        let bytes = self.clone().into_bytes();
        let len = bytes.len() as u64;
        len.write_bin(writer)?;
        writer.write(bytes.as_slice())?;
        Ok(())
    }
}

impl<B: SimpleBinRepr> SimpleBinRepr for Vec<B> {
    fn read_bin<R: Read>(reader: &mut R) -> Result<Self> {
        let len = u64::read_bin(reader)? as usize;
        let mut vec: Vec<B> = Vec::with_capacity(len);
        for _ in 0..len {
            vec.push(B::read_bin(reader)?);
        }
        Ok(vec)
    }

    fn write_bin<W: Write>(&self, writer: &mut W) -> Result<()> {
        (self.len() as u64).write_bin(writer)?;
        for e in self {
            e.write_bin(writer)?;
        }
        Ok(())
    }
}

impl SimpleBinRepr for PathBuf {
    fn read_bin<R: Read>(reader: &mut R) -> Result<Self> {
        let string = String::read_bin(reader)?;
        let pb = PathBuf::from(string);
        Ok(pb)
    }

    fn write_bin<W: Write>(&self, writer: &mut W) -> Result<()> {
        let string = self.to_str().context("Path not valid utf8")?.to_string();
        string.write_bin(writer)
    }
}

impl<B1: SimpleBinRepr, B2: SimpleBinRepr> SimpleBinRepr for (B1, B2) {
    fn read_bin<R: Read>(reader: &mut R) -> Result<Self> {
        Ok((B1::read_bin(reader)?, B2::read_bin(reader)?))
    }

    fn write_bin<W: Write>(&self, writer: &mut W) -> Result<()> {
        self.0.write_bin(writer)?;
        self.1.write_bin(writer)
    }
}

fn read_bytes_const<R: Read, const N: usize>(reader: &mut R) -> Result<[u8; N]> {
    let mut buffer = [0_u8; N];
    reader.read_exact(&mut buffer)?;
    Ok(buffer)
}
fn read_bytes<R: Read>(reader: &mut R, n: usize) -> Result<Vec<u8>> {
    let mut buffer = vec![0_u8; n];
    reader.read_exact(&mut buffer)?;
    Ok(buffer)
}

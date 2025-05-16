use anyhow::{Context, Result, anyhow};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
};

use crate::index::Index;

pub trait SimpleBinRepr: Sized {
    fn read_bin<R: Read>(reader: &mut R) -> Result<Self>;
    fn write_bin<W: Write>(&self, writer: &mut W) -> Result<()>;
    #[allow(unused)]
    fn simple_bin_vec(&self) -> Result<Vec<u8>> {
        let mut buffer = vec![];
        self.write_bin(&mut buffer)?;
        Ok(buffer)
    }
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
impl SimpleBinRepr for u32 {
    fn read_bin<R: Read>(reader: &mut R) -> Result<Self> {
        let bytes = read_bytes_const::<R, 4>(reader)?;
        Ok(u32::from_le_bytes(bytes))
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

impl SimpleBinRepr for Index {
    fn read_bin<R: Read>(reader: &mut R) -> Result<Self> {
        let revision = u32::read_bin(reader)?;
        let variant = u32::read_bin(reader)?;
        let hash_indices: Vec<(u64, u64)> = Vec::read_bin(reader)?;
        let hashes: Vec<[u8; 32]> = Vec::read_bin(reader)?;
        let sizes: Vec<u64> = Vec::read_bin(reader)?;
        let mapping_indices: Vec<(u64, u64)> = Vec::read_bin(reader)?;
        let maps: Vec<PathBuf> = Vec::read_bin(reader)?;
        let empty_dirs: Vec<PathBuf> = Vec::read_bin(reader)?;

        if hash_indices.len() != hashes.len() {
            return Err(anyhow!("Malformed index"));
        }
        if hash_indices.len() != sizes.len() {
            return Err(anyhow!("Malformed index"));
        }
        if mapping_indices.len() != maps.len() {
            return Err(anyhow!("Malformed index"));
        }

        let hm_hashes: HashMap<(u64, u64), [u8; 32]> =
            hash_indices.clone().into_iter().zip(hashes).collect();
        let hm_sizes: HashMap<(u64, u64), u64> = hash_indices.into_iter().zip(sizes).collect();
        let hm_mapping: HashMap<PathBuf, (u64, u64)> =
            maps.into_iter().zip(mapping_indices).collect();
        Ok(Self {
            hashes: hm_hashes,
            sizes: hm_sizes,
            mapping: hm_mapping,
            revision,
            variant,
            empty_dirs,
        })
    }

    fn write_bin<W: Write>(&self, writer: &mut W) -> Result<()> {
        let mut hash_indices = vec![];
        let mut map_indices = vec![];
        let mut hashes = vec![];
        let mut sizes = vec![];
        let mut maps = vec![];
        for (hi, hash) in &self.hashes {
            hash_indices.push(*hi);
            let size = self
                .sizes
                .get(hi)
                .context("Index missing size information")?;
            sizes.push(*size);
            hashes.push(*hash);
        }
        for (path, mi) in &self.mapping {
            map_indices.push(*mi);
            maps.push(path.clone());
        }
        self.revision.write_bin(writer)?;
        self.variant.write_bin(writer)?;
        hash_indices.write_bin(writer)?;
        hashes.write_bin(writer)?;
        sizes.write_bin(writer)?;
        map_indices.write_bin(writer)?;
        maps.write_bin(writer)?;
        self.empty_dirs.write_bin(writer)
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

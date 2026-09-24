use std::fmt;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::chunk::{to_hex, FileChunk, Sha256Hash};
use crate::chunker::{chunk_path, chunk_reader};
use crate::error::Result;
use crate::manifest::FileManifest;
use crate::CHUNK_SIZE;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePrimitive {
    manifest: FileManifest,
    chunks: Vec<FileChunk>,
}

impl FilePrimitive {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_path_with_chunk_size(path, CHUNK_SIZE)
    }

    pub fn from_path_with_chunk_size(path: impl AsRef<Path>, chunk_size: usize) -> Result<Self> {
        let mut chunks = Vec::new();
        let manifest = chunk_path(path, chunk_size, |c| {
            chunks.push(c);
            Ok(())
        })?;
        Ok(FilePrimitive { manifest, chunks })
    }

    pub fn from_reader<R: Read>(
        name: impl Into<String>,
        reader: R,
        chunk_size: usize,
    ) -> Result<Self> {
        let mut chunks = Vec::new();
        let manifest = chunk_reader(name.into(), reader, chunk_size, |c| {
            chunks.push(c);
            Ok(())
        })?;
        Ok(FilePrimitive { manifest, chunks })
    }

    pub fn from_parts(manifest: FileManifest, mut chunks: Vec<FileChunk>) -> Result<Self> {
        chunks.sort_by_key(|c| c.index);
        manifest.verify_chunks(&chunks)?;
        Ok(FilePrimitive { manifest, chunks })
    }

    pub fn manifest(&self) -> &FileManifest {
        &self.manifest
    }

    pub fn chunks(&self) -> &[FileChunk] {
        &self.chunks
    }

    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    pub fn size(&self) -> u64 {
        self.manifest.size
    }

    pub fn checksum(&self) -> &Sha256Hash {
        &self.manifest.checksum
    }

    pub fn verify(&self) -> Result<()> {
        self.manifest.verify_chunks(&self.chunks)
    }

    pub fn write_to<W: Write>(&self, writer: W) -> Result<()> {
        let mut writer = BufWriter::new(writer);
        for chunk in &self.chunks {
            writer.write_all(&chunk.data)?;
        }
        writer.flush()?;
        Ok(())
    }

    pub fn write_to_path(&self, path: impl AsRef<Path>) -> Result<()> {
        self.write_to(File::create(path)?)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.manifest.size as usize);
        for chunk in &self.chunks {
            out.extend_from_slice(&chunk.data);
        }
        out
    }

    pub fn into_parts(self) -> (FileManifest, Vec<FileChunk>) {
        (self.manifest, self.chunks)
    }
}

impl fmt::Display for FilePrimitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FilePrimitive {{ name: {}, size: {} octets, chunks: {}, checksum: {} }}",
            self.manifest.name,
            self.manifest.size,
            self.chunks.len(),
            to_hex(&self.manifest.checksum)
        )
    }
}

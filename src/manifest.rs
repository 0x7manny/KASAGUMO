use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::chunk::{to_hex, ChunkId, FileChunk, Sha256Hash};
use crate::error::{Result, StorageError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileManifest {
    pub name: String,
    pub size: u64,
    pub chunk_size: u64,
    pub chunks: Vec<ChunkId>,
    pub checksum: Sha256Hash,
}

impl FileManifest {
    pub fn verify_chunks(&self, chunks: &[FileChunk]) -> Result<()> {
        if chunks.len() != self.chunks.len() {
            return Err(StorageError::ChunkCount {
                expected: self.chunks.len(),
                actual: chunks.len(),
            });
        }

        let mut hasher = Sha256::new();
        let mut size = 0u64;

        for (position, (chunk, expected_id)) in chunks.iter().zip(&self.chunks).enumerate() {
            if chunk.index != position as u64 || chunk.id != *expected_id {
                return Err(StorageError::ChunkMismatch { position });
            }
            chunk.verify()?;
            size += chunk.data.len() as u64;
            hasher.update(&chunk.data);
        }

        if size != self.size {
            return Err(StorageError::SizeMismatch {
                expected: self.size,
                actual: size,
            });
        }
        let checksum: Sha256Hash = hasher.finalize().into();
        if checksum != self.checksum {
            return Err(StorageError::FileChecksumMismatch);
        }
        Ok(())
    }
}

impl fmt::Display for FileManifest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FileManifest {{ name: {}, size: {} octets, chunks: {}, checksum: {} }}",
            self.name,
            self.size,
            self.chunks.len(),
            to_hex(&self.checksum)
        )
    }
}

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Result, StorageError};

pub type Sha256Hash = [u8; 32];

pub fn calculate_checksum(data: &[u8]) -> Sha256Hash {
    Sha256::digest(data).into()
}

pub(crate) fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChunkId(pub Sha256Hash);

impl ChunkId {
    pub fn of(data: &[u8]) -> Self {
        ChunkId(calculate_checksum(data))
    }

    pub fn short(&self) -> String {
        to_hex(&self.0[..8])
    }
}

impl fmt::Display for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&to_hex(&self.0))
    }
}

impl fmt::Debug for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChunkId({}…)", self.short())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChunk {
    pub id: ChunkId,
    pub index: u64,
    pub data: Vec<u8>,
}

impl FileChunk {
    pub fn new(index: u64, data: Vec<u8>) -> Self {
        let id = ChunkId::of(&data);
        FileChunk { id, index, data }
    }

    pub fn verify(&self) -> Result<()> {
        let actual = ChunkId::of(&self.data);
        if actual == self.id {
            Ok(())
        } else {
            Err(StorageError::CorruptedChunk {
                index: self.index,
                expected: self.id,
                actual,
            })
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl fmt::Debug for FileChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileChunk")
            .field("id", &self.id)
            .field("index", &self.index)
            .field("len", &self.data.len())
            .finish()
    }
}

impl fmt::Display for FileChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FileChunk {{ id: {}, index: {}, taille: {} octets }}",
            self.id.short(),
            self.index,
            self.data.len()
        )
    }
}

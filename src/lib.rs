mod chunk;
mod chunker;
mod error;
mod manifest;
mod primitive;

pub use chunk::{calculate_checksum, ChunkId, FileChunk, Sha256Hash};
pub use chunker::{chunk_path, chunk_reader};
pub use error::{Result, StorageError};
pub use manifest::FileManifest;
pub use primitive::FilePrimitive;

pub const CHUNK_SIZE: usize = 1024 * 1024;

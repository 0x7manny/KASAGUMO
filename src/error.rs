use std::io;

use crate::chunk::ChunkId;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("erreur d'entrée/sortie : {0}")]
    Io(#[from] io::Error),

    #[error("nom de fichier invalide : {0}")]
    InvalidName(String),

    #[error("la taille de chunk doit être supérieure à 0")]
    InvalidChunkSize,

    #[error("chunk {index} corrompu : attendu {expected}, obtenu {actual}")]
    CorruptedChunk {
        index: u64,
        expected: ChunkId,
        actual: ChunkId,
    },

    #[error("le chunk à la position {position} ne correspond pas au manifeste")]
    ChunkMismatch { position: usize },

    #[error("nombre de chunks incorrect : attendu {expected}, obtenu {actual}")]
    ChunkCount { expected: usize, actual: usize },

    #[error("taille incorrecte : attendu {expected} octets, obtenu {actual}")]
    SizeMismatch { expected: u64, actual: u64 },

    #[error("le checksum global du fichier ne correspond pas")]
    FileChecksumMismatch,
}

pub type Result<T> = std::result::Result<T, StorageError>;

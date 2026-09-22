//Take a selected file and prepare it

use std::fs::File as StdFile;
use std::io::Read;
use std::path::Path;
use sha2::{Sha256, Digest};

static CHUNK_SIZE: usize = 1024;

fn main() {
    FilePrimitive::createFromFile("./assets/kasagumo-logo.png");
}

struct File {
    name: String,
    size: u64,
    chunks: Vec<u64>,
    checksum: [u8; 32],
}

struct FileChunk {
    chunk_id: u64,
    data: Vec<u8>,
    chunk_index: usize,
    chunk_checksum: [u8; 32],
}

struct FilePrimitive {
    name: String,
    size: u64,
    chunks: Vec<FileChunk>,
    checksum: [u8; 32],
}

fn calculate_checksum(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);

    let result = hasher.finalize();

    let mut checksum = [0u8; 32];
    checksum.copy_from_slice(&result);

    checksum
}

fn make_chunk_id(
    idx: u64,
    chunk_checksum: &[u8; 32],
    file_checksum: &[u8; 32],
) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(idx.to_le_bytes());
    hasher.update(chunk_checksum);
    hasher.update(file_checksum);
    let result = hasher.finalize();
    u64::from_le_bytes(result[0..8].try_into().unwrap())
}

impl FilePrimitive {
    fn new(
        name: String,
        size: u64,
        chunks: Vec<FileChunk>,
        checksum: [u8; 32],
    ) -> Self {
        FilePrimitive {
            name,
            size,
            chunks,
            checksum,
        }
    }

    fn createFromFile(path: &str) -> Self {
        let mut file =
            StdFile::open(Path::new(path)).expect("Failed to open file");
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .expect("Failed to read file");
        let file_size = buffer.len() as u64;
        let file_checksum = calculate_checksum(&buffer);
        let mut chunks = Vec::new();
        for (i, chunk) in buffer.chunks(CHUNK_SIZE).enumerate() {
            println!("Creating chunk {}", i);
            let bytes = chunk.to_vec();
            let chunk_checksum = calculate_checksum(&bytes);
            let chunk_index = i;
        }

        FilePrimitive::new(
            String::from(path),
            file_size,
            chunks,
            file_checksum,
        )
    }
}
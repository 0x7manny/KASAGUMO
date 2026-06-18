//Take a selected file and prepare it
use std::fs::File as StdFile;
use std::io::Read;
use std::path::Path;
use sha2::{Digest, Sha256};

static CHUNK_SIZE: usize = 1024 * 1024;

fn main() {
    let file_primitive = FilePrimitive::createFromFile("./assets/kasagumo-logo.png");
    println!("{}", file_primitive.to_string());
}

#[derive(Debug, Clone, PartialEq)]
struct File {
    name: String,
    size: u64,
    chunks: Vec<u64>,
    checksum: [u8; 32],
}

#[derive(Debug, Clone, PartialEq)]
struct FileChunk {
    chunk_id: u64,
    data: Vec<u8>,
    chunk_index: usize,
    chunk_checksum: [u8; 32],
}

#[derive(Debug, Clone, PartialEq)]
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
        let mut file = StdFile::open(Path::new(path)).expect("Failed to open file");
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .expect("Failed to read file");

        let file_size = buffer.len() as u64;
        let file_checksum = calculate_checksum(&buffer);
        let mut chunks = Vec::new();

        for (i, chunk) in buffer.chunks(CHUNK_SIZE).enumerate() {
            let bytes = chunk.to_vec();
            let chunk_checksum = calculate_checksum(&bytes);
            let chunk_index = i;
            let chunk_id = make_chunk_id(i as u64, &chunk_checksum, &file_checksum);

            let file_chunk = FileChunk {
                chunk_id,
                data: bytes,
                chunk_index,
                chunk_checksum,
            };

            chunks.push(file_chunk);
        }

        FilePrimitive::new(
            String::from(path),
            file_size,
            chunks,
            file_checksum,
        )
    }

    fn get_file_struct(&self) -> File {
        let chunk_ids: Vec<u64> = self.chunks.iter().map(|chunk| chunk.chunk_id).collect();

        File {
            name: self.name.clone(),
            size: self.size,
            chunks: chunk_ids,
            checksum: self.checksum,
        }
    }

    fn get_chunks(&self) -> &Vec<FileChunk> {
        &self.chunks
    }

    fn to_string(&self) -> String {
        format!(
            "FilePrimitive (name: {}, size: {}, chunks: {}, checksum: {:x?})",
            self.name,
            self.size,
            self.chunks.len(),
            self.checksum
        )
    }
}
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::chunk::FileChunk;
use crate::error::{Result, StorageError};
use crate::manifest::FileManifest;

fn read_full<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

pub fn chunk_reader<R, F>(
    name: String,
    mut reader: R,
    chunk_size: usize,
    mut on_chunk: F,
) -> Result<FileManifest>
where
    R: Read,
    F: FnMut(FileChunk) -> Result<()>,
{
    if chunk_size == 0 {
        return Err(StorageError::InvalidChunkSize);
    }

    let mut buf = vec![0u8; chunk_size];
    let mut file_hasher = Sha256::new();
    let mut ids = Vec::new();
    let mut size = 0u64;
    let mut index = 0u64;

    loop {
        let n = read_full(&mut reader, &mut buf)?;
        if n == 0 {
            break;
        }

        let data = buf[..n].to_vec();
        file_hasher.update(&data);
        size += n as u64;

        let chunk = FileChunk::new(index, data);
        ids.push(chunk.id);
        on_chunk(chunk)?;
        index += 1;

        if n < chunk_size {
            break;
        }
    }

    Ok(FileManifest {
        name,
        size,
        chunk_size: chunk_size as u64,
        chunks: ids,
        checksum: file_hasher.finalize().into(),
    })
}

fn file_name_of(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .ok_or_else(|| StorageError::InvalidName(path.display().to_string()))
}

pub fn chunk_path<F>(path: impl AsRef<Path>, chunk_size: usize, on_chunk: F) -> Result<FileManifest>
where
    F: FnMut(FileChunk) -> Result<()>,
{
    let path = path.as_ref();
    let name = file_name_of(path)?;
    let reader = BufReader::new(File::open(path)?);
    chunk_reader(name, reader, chunk_size, on_chunk)
}

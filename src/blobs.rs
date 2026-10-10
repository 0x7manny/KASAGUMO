//! Blocs adressés par leur contenu (SHA-256), stockés un fichier par bloc.

use std::path::PathBuf;

use anyhow::Context;
use kasagumo::ChunkId;

pub struct Blobs(PathBuf);

impl Blobs {
    pub fn open(dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&dir).with_context(|| format!("impossible de créer {}", dir.display()))?;
        Ok(Self(dir))
    }

    pub fn put(&self, data: &[u8]) -> anyhow::Result<String> {
        let id = ChunkId::of(data).to_string();
        let tmp = self.0.join(format!("{id}.tmp"));
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, self.0.join(&id))?;
        Ok(id)
    }

    /// Les blocs présents sur ce nœud.
    pub fn ids(&self) -> anyhow::Result<Vec<String>> {
        let names = std::fs::read_dir(&self.0)?.filter_map(|e| e.ok()?.file_name().into_string().ok());
        Ok(names.filter(|name| is_id(name)).collect())
    }

    /// `None` si le bloc est absent ou corrompu.
    pub fn get(&self, id: &str) -> anyhow::Result<Option<Vec<u8>>> {
        anyhow::ensure!(is_id(id), "identifiant invalide : {id}");
        match std::fs::read(self.0.join(id)) {
            Ok(data) => Ok((ChunkId::of(&data).to_string() == id).then_some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// Un identifiant de bloc : 64 chiffres hexadécimaux (évite aussi de sortir du dossier).
pub fn is_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn decode(hex: &str) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(hex.is_ascii() && hex.len().is_multiple_of(2), "hexadécimal invalide");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).context("hexadécimal invalide"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_aller_retour() {
        let data = [0u8, 1, 127, 128, 255];
        assert_eq!(decode(&encode(&data)).unwrap(), data);
        assert!(decode("abc").is_err() && decode("zz").is_err());
    }

    #[test]
    fn identifiants_valides() {
        assert!(is_id(&"a".repeat(64)));
        assert!(!is_id("../../etc/passwd") && !is_id(&"g".repeat(64)));
    }
}

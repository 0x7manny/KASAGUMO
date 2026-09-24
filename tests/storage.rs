use std::io::{self, Cursor, Read};

use kasagumo::*;

fn data(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

fn split(bytes: &[u8], chunk_size: usize) -> FilePrimitive {
    FilePrimitive::from_reader("test.bin", Cursor::new(bytes.to_vec()), chunk_size).unwrap()
}

#[test]
fn aller_retour() {
    let original = data(10_000);
    let f = split(&original, 1024);
    assert_eq!(f.to_bytes(), original);
    assert_eq!(f.size(), 10_000);
    assert_eq!(f.chunks().len(), 10); // 9 chunks pleins + 1 partiel
    f.verify().unwrap();
}

#[test]
fn checksum_global_egal_au_sha256_du_fichier() {
    let original = data(5_000);
    let f = split(&original, 700);
    assert_eq!(*f.checksum(), calculate_checksum(&original));
}

#[test]
fn fichier_vide() {
    let f = split(&[], 1024);
    assert_eq!(f.size(), 0);
    assert!(f.chunks().is_empty());
    assert_eq!(*f.checksum(), calculate_checksum(&[]));
    f.verify().unwrap();
}

#[test]
fn taille_multiple_exact_de_chunk_size() {
    let f = split(&data(4096), 1024);
    assert_eq!(f.chunks().len(), 4); // pas de chunk vide en trop
    assert!(f.chunks().iter().all(|c| c.len() == 1024));
}

#[test]
fn chunk_size_plus_un() {
    let f = split(&data(1025), 1024);
    assert_eq!(f.chunks().len(), 2);
    assert_eq!(f.chunks()[1].len(), 1);
}

#[test]
fn taille_reelle_de_chunk() {
    let f = split(&data(CHUNK_SIZE + 1), CHUNK_SIZE);
    assert_eq!(f.chunks().len(), 2);
    f.verify().unwrap();
}

#[test]
fn chunk_size_zero_refuse() {
    let r = FilePrimitive::from_reader("x", Cursor::new(vec![1, 2, 3]), 0);
    assert!(matches!(r, Err(StorageError::InvalidChunkSize)));
}

#[test]
fn chunk_altere_detecte() {
    let (manifest, mut chunks) = split(&data(3000), 1000).into_parts();
    chunks[1].data[0] ^= 0xFF; // un pair malveillant modifie un octet
    assert!(chunks[1].verify().is_err());
    assert!(FilePrimitive::from_parts(manifest, chunks).is_err());
}

#[test]
fn chunk_remplace_detecte() {
    // Un pair envoie un chunk valide... mais qui n'est pas le bon.
    let (manifest, mut chunks) = split(&data(3000), 1000).into_parts();
    chunks[1] = FileChunk::new(1, vec![0u8; 1000]);
    assert!(matches!(
        FilePrimitive::from_parts(manifest, chunks),
        Err(StorageError::ChunkMismatch { position: 1 })
    ));
}

#[test]
fn chunk_manquant_detecte() {
    let (manifest, mut chunks) = split(&data(3000), 1000).into_parts();
    chunks.pop();
    assert!(matches!(
        FilePrimitive::from_parts(manifest, chunks),
        Err(StorageError::ChunkCount { .. })
    ));
}

#[test]
fn reconstruction_dans_le_desordre() {
    // Les chunks arrivent de plusieurs pairs, dans n'importe quel ordre.
    let original = data(5000);
    let (manifest, mut chunks) = split(&original, 1000).into_parts();
    chunks.reverse();
    let f = FilePrimitive::from_parts(manifest, chunks).unwrap();
    assert_eq!(f.to_bytes(), original);
}

#[test]
fn deduplication_meme_contenu_meme_id() {
    // Deux fichiers différents qui partagent un bloc identique.
    let bloc = vec![42u8; 1000];
    let mut a = bloc.clone();
    a.extend(vec![1u8; 1000]);
    let mut b = bloc.clone();
    b.extend(vec![2u8; 1000]);

    let fa = split(&a, 1000);
    let fb = split(&b, 1000);
    assert_eq!(fa.chunks()[0].id, fb.chunks()[0].id);
    assert_ne!(fa.chunks()[1].id, fb.chunks()[1].id);
}

/// Lecteur qui ne renvoie que quelques octets à la fois,
/// comme un socket réseau ou un pipe.
struct LecteurLent {
    inner: Cursor<Vec<u8>>,
}

impl Read for LecteurLent {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let max = buf.len().min(7);
        self.inner.read(&mut buf[..max])
    }
}

#[test]
fn lectures_partielles_gerees() {
    let original = data(5000);
    let lent = LecteurLent {
        inner: Cursor::new(original.clone()),
    };
    let f = FilePrimitive::from_reader("lent.bin", lent, 1000).unwrap();
    // Sans read_full, on aurait des centaines de chunks de 7 octets.
    assert_eq!(f.chunks().len(), 5);
    assert_eq!(f.to_bytes(), original);
}

#[test]
fn nom_sans_chemin_et_ecriture_disque() {
    let dir = std::env::temp_dir().join("kasagumo-test");
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("source.bin");
    let dst = dir.join("copie.bin");
    let original = data(2500);
    std::fs::write(&src, &original).unwrap();

    let f = FilePrimitive::from_path_with_chunk_size(&src, 1000).unwrap();
    assert_eq!(f.name(), "source.bin");

    f.write_to_path(&dst).unwrap();
    assert_eq!(std::fs::read(&dst).unwrap(), original);
}

#[test]
fn fichier_introuvable_renvoie_une_erreur() {
    let r = FilePrimitive::from_path("/chemin/qui/n/existe/pas.png");
    assert!(matches!(r, Err(StorageError::Io(_))));
}

#[test]
fn streaming_arrete_si_callback_echoue() {
    let mut vus = 0;
    let r = chunk_reader("x".into(), Cursor::new(data(5000)), 1000, |_| {
        vus += 1;
        if vus == 2 {
            Err(StorageError::Io(io::Error::other("disque plein")))
        } else {
            Ok(())
        }
    });
    assert!(r.is_err());
    assert_eq!(vus, 2);
}

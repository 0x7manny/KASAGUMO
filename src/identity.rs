//! Identité des nœuds : chaque nœud a sa clé Ed25519, et la clé du cluster signe un certificat
//! pour chaque nœud admis. Deux nœuds se reconnaissent en signant la session TLS (channel binding) :
//! aucun secret partagé ne circule, et la signature d'un rôle ne peut pas être rejouée pour l'autre.

use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::Context;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::blobs::{decode, encode};
use crate::secure::SessionKey;

const NODE_KEY: &str = "node.key";
const NODE_CERT: &str = "node.cert";
const CLUSTER_KEY: &str = "cluster.key";
const CLUSTER_PUB: &str = "cluster.pub";

#[derive(Clone)]
pub struct Identity {
    key: SigningKey,
    cluster: VerifyingKey,
    cert: Signature,
}

fn cert_message(node: &VerifyingKey) -> Vec<u8> {
    [b"kgo-cert-v1".as_slice(), node.as_bytes()].concat()
}

fn session_message(role: &str, session: &SessionKey) -> Vec<u8> {
    [b"kgo-session-v1".as_slice(), role.as_bytes(), session].concat()
}

/// Identifiant court d'un nœud, dérivé de sa clé publique.
pub fn node_id(node: &VerifyingKey) -> String {
    encode(&Sha256::digest(node.as_bytes())[..8])
}

fn bytes<const N: usize>(hex: &str) -> anyhow::Result<[u8; N]> {
    decode(hex.trim())?.try_into().map_err(|_| anyhow::anyhow!("longueur invalide ({N} octets attendus)"))
}

fn read_signing_key(path: &Path) -> anyhow::Result<SigningKey> {
    let hex = std::fs::read_to_string(path).with_context(|| format!("impossible de lire {}", path.display()))?;
    Ok(SigningKey::from_bytes(&bytes(&hex)?))
}

/// Écrit un secret lisible par son propriétaire seulement.
fn write_secret(path: &Path, key: &SigningKey) -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    file.write_all(encode(&key.to_bytes()).as_bytes())?;
    Ok(())
}

fn load_or_create_node_key(dir: &Path) -> anyhow::Result<SigningKey> {
    let path = dir.join(NODE_KEY);
    if path.exists() {
        return read_signing_key(&path);
    }
    std::fs::create_dir_all(dir)?;
    let key = SigningKey::from_bytes(&rand::random());
    write_secret(&path, &key)?;
    Ok(key)
}

/// La clé publique de ce nœud (créée au premier appel), à remettre à qui administre le cluster.
pub fn node_pubkey(dir: &Path) -> anyhow::Result<String> {
    Ok(encode(load_or_create_node_key(dir)?.verifying_key().as_bytes()))
}

/// Crée un cluster dont ce nœud est le premier membre, et détient la clé qui admet les suivants.
pub fn init_cluster(dir: &Path) -> anyhow::Result<String> {
    anyhow::ensure!(!dir.join(CLUSTER_PUB).exists(), "ce nœud fait déjà partie d'un cluster");
    let node = load_or_create_node_key(dir)?;
    let cluster = SigningKey::from_bytes(&rand::random());
    write_secret(&dir.join(CLUSTER_KEY), &cluster)?;
    install(dir, &cluster.verifying_key(), &cluster.sign(&cert_message(&node.verifying_key())))?;
    Ok(node_id(&node.verifying_key()))
}

/// Délivre le certificat d'admission d'un nœud (clé publique en hexadécimal) ; réservé au nœud qui a créé le cluster.
pub fn admit(dir: &Path, node_pubkey: &str) -> anyhow::Result<String> {
    let cluster = read_signing_key(&dir.join(CLUSTER_KEY))
        .context("ce nœud n'a pas créé le cluster : l'admission se fait là où `kgo cluster init` a été lancé")?;
    let node = VerifyingKey::from_bytes(&bytes(node_pubkey)?).context("clé publique invalide")?;
    let cert = cluster.sign(&cert_message(&node));
    Ok(format!("{}:{}", encode(cluster.verifying_key().as_bytes()), encode(&cert.to_bytes())))
}

/// Enregistre le certificat reçu : ce nœud rejoint le cluster qui l'a délivré.
pub fn enroll(dir: &Path, certificate: &str) -> anyhow::Result<String> {
    anyhow::ensure!(!dir.join(CLUSTER_PUB).exists(), "ce nœud fait déjà partie d'un cluster");
    let (cluster, cert) = certificate.trim().split_once(':').context("certificat invalide")?;
    let cluster = VerifyingKey::from_bytes(&bytes(cluster)?).context("clé du cluster invalide")?;
    let cert = Signature::from_bytes(&bytes(cert)?);
    let node = load_or_create_node_key(dir)?.verifying_key();
    cluster.verify(&cert_message(&node), &cert).context("ce certificat n'a pas été délivré pour ce nœud")?;
    install(dir, &cluster, &cert)?;
    Ok(node_id(&node))
}

fn install(dir: &Path, cluster: &VerifyingKey, cert: &Signature) -> anyhow::Result<()> {
    std::fs::write(dir.join(CLUSTER_PUB), encode(cluster.as_bytes()))?;
    std::fs::write(dir.join(NODE_CERT), encode(&cert.to_bytes()))?;
    Ok(())
}

impl Identity {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let missing = "ce nœud ne fait partie d'aucun cluster (`kgo cluster init`, ou `kgo node enroll <certificat>`)";
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).context(missing);
        let key = read_signing_key(&dir.join(NODE_KEY)).context(missing)?;
        let cluster = VerifyingKey::from_bytes(&bytes(&read(CLUSTER_PUB)?)?).context("cluster.pub invalide")?;
        let cert = Signature::from_bytes(&bytes(&read(NODE_CERT)?)?);
        cluster.verify(&cert_message(&key.verifying_key()), &cert).context("node.cert ne correspond pas à ce nœud")?;
        Ok(Self { key, cluster, cert })
    }

    pub fn node_id(&self) -> String {
        node_id(&self.key.verifying_key())
    }

    /// Ligne envoyée à l'ouverture d'une connexion : clé publique, certificat, signature de la session.
    pub fn handshake(&self, role: &str, session: &SessionKey) -> String {
        let signature = self.key.sign(&session_message(role, session));
        format!(
            "{} {} {}",
            encode(self.key.verifying_key().as_bytes()),
            encode(&self.cert.to_bytes()),
            encode(&signature.to_bytes())
        )
    }

    /// L'identifiant du pair si sa ligne prouve qu'il est membre de ce cluster et tient cette session.
    pub fn verify(&self, line: &str, role: &str, session: &SessionKey) -> Option<String> {
        let mut fields = line.split_whitespace();
        let node = VerifyingKey::from_bytes(&bytes(fields.next()?).ok()?).ok()?;
        let cert = Signature::from_bytes(&bytes(fields.next()?).ok()?);
        let signature = Signature::from_bytes(&bytes(fields.next()?).ok()?);
        self.cluster.verify(&cert_message(&node), &cert).ok()?;
        node.verify(&session_message(role, session), &signature).ok()?;
        Some(node_id(&node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kgo-identity-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        dir
    }

    /// Un cluster de deux nœuds : `a` le crée, `b` est admis.
    fn cluster(name: &str) -> (Identity, Identity) {
        let (a, b) = (dir(&format!("{name}-a")), dir(&format!("{name}-b")));
        init_cluster(&a).unwrap();
        let cert = admit(&a, &node_pubkey(&b).unwrap()).unwrap();
        enroll(&b, &cert).unwrap();
        (Identity::load(&a).unwrap(), Identity::load(&b).unwrap())
    }

    #[test]
    fn deux_membres_se_reconnaissent() {
        let (a, b) = cluster("ok");
        let session = [9; 32];
        assert_eq!(b.verify(&a.handshake("client", &session), "client", &session), Some(a.node_id()));
        assert_eq!(a.verify(&b.handshake("server", &session), "server", &session), Some(b.node_id()));
    }

    #[test]
    fn la_signature_ne_vaut_que_pour_sa_session_et_son_role() {
        let (a, b) = cluster("replay");
        let line = a.handshake("client", &[1; 32]);
        assert_eq!(b.verify(&line, "client", &[2; 32]), None, "autre session");
        assert_eq!(b.verify(&line, "server", &[1; 32]), None, "autre rôle");
        assert_eq!(b.verify("", "client", &[1; 32]), None);
        assert_eq!(b.verify(&line.replacen('a', "b", 1), "client", &[1; 32]), None, "ligne altérée");
    }

    #[test]
    fn un_autre_cluster_est_refuse() {
        let (a, _) = cluster("one");
        let (x, _) = cluster("two");
        let session = [3; 32];
        assert_eq!(a.verify(&x.handshake("client", &session), "client", &session), None);
    }

    #[test]
    fn admission_et_adhesion_controlees() {
        let (a, b) = (dir("ctl-a"), dir("ctl-b"));
        init_cluster(&a).unwrap();
        assert!(init_cluster(&a).is_err(), "déjà dans un cluster");
        assert!(admit(&b, &node_pubkey(&a).unwrap()).is_err(), "b n'a pas la clé du cluster");
        assert!(Identity::load(&b).is_err(), "b n'est pas enrôlé");

        // un certificat délivré pour un autre nœud n'est pas accepté
        let cert = admit(&a, &node_pubkey(&dir("ctl-c")).unwrap()).unwrap();
        assert!(enroll(&b, &cert).is_err());
    }
}

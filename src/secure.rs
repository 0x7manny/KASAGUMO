//! Lien chiffré entre nœuds : TLS 1.3 avec un certificat éphémère non vérifié,
//! puis une preuve HMAC du token du cluster liée à la session (channel binding).
//! Le token ne circule jamais, et un homme du milieu ne peut pas rejouer la preuve.

use std::sync::Arc;

use hmac::{Hmac, Mac};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, ring};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, ConnectionCommon, DigitallySignedStruct, ServerConfig, SignatureScheme, version};
use sha2::Sha256;
use tokio_rustls::{TlsAcceptor, TlsConnector};

pub const SERVER_NAME: &str = "kasagumo";

/// Secret dérivé de la session TLS, identique des deux côtés.
pub type SessionKey = [u8; 32];

pub fn acceptor() -> anyhow::Result<TlsAcceptor> {
    let cert = rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_string()])?;
    let key = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
    let config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_protocol_versions(&[&version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key.into())?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub fn connector() -> anyhow::Result<TlsConnector> {
    let provider = Arc::new(ring::default_provider());
    let config = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCert(provider)))
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Preuve que `role` connaît `token` pour cette session ; vide si le nœud n'a pas de token.
pub fn proof(token: &str, key: &SessionKey, role: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("HMAC accepte toute longueur");
    mac.update(key);
    mac.update(role.as_bytes());
    mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// Comparaison en temps constant.
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |diff, (x, y)| diff | (x ^ y)) == 0
}

pub fn session_key<D>(conn: &ConnectionCommon<D>) -> anyhow::Result<SessionKey> {
    let mut key = [0; 32];
    conn.export_keying_material(&mut key, b"kgo", None)?;
    Ok(key)
}

/// L'identité du pair est prouvée par le token, pas par le certificat.
#[derive(Debug)]
struct AnyCert(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, _: &[u8], _: &CertificateDer<'_>, _: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::PeerIncompatible(rustls::PeerIncompatible::Tls12NotOffered))
    }

    fn verify_tls13_signature(&self, msg: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(msg, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

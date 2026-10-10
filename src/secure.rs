//! Lien chiffré entre nœuds : TLS 1.3 avec un certificat éphémère non vérifié,
//! puis une preuve d'identité des deux nœuds liée à la session (voir `identity.rs`).

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, ring};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, ConnectionCommon, DigitallySignedStruct, ServerConfig, SignatureScheme, version};
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

pub fn session_key<D>(conn: &ConnectionCommon<D>) -> anyhow::Result<SessionKey> {
    let mut key = [0; 32];
    conn.export_keying_material(&mut key, b"kgo", None)?;
    Ok(key)
}

/// L'identité du pair est prouvée par `identity.rs`, pas par le certificat TLS.
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

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UnixStream};
use rustls::pki_types::ServerName;

use crate::daemon::socket_path;
use crate::identity::Identity;
use crate::protocol::{Request, Response};
use crate::secure::{self, SessionKey};

const PEER_TIMEOUT: Duration = Duration::from_secs(2);
/// Les transferts de blocs (1 Mio chacun) ont droit à plus de temps.
const BLOB_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn send(data_dir: &Path, request: &Request) -> anyhow::Result<Response> {
    let path = socket_path(data_dir);
    let stream = UnixStream::connect(&path).await.with_context(|| {
        format!(
            "aucun nœud ne répond sur {} (lance `kgo node start`)",
            path.display()
        )
    })?;
    exchange(stream, None, request).await
}

/// Envoie la requête au nœud local, ou à `on` par son intermédiaire.
pub async fn send_on(data_dir: &Path, on: Option<&str>, request: Request) -> anyhow::Result<Response> {
    match on {
        Some(addr) => send(data_dir, &Request::Forward { addr: addr.to_string(), request: Box::new(request) }).await,
        None => send(data_dir, &request).await,
    }
}

/// Envoie une requête à un nœud distant, avec un délai maximum. Sans `identity`, on reste anonyme
/// (le pair ne répond alors qu'à `Info`) et on ne vérifie pas qui répond.
pub async fn send_to_peer(addr: &str, identity: Option<&Identity>, request: &Request) -> anyhow::Result<Response> {
    let attempt = async {
        let tcp = TcpStream::connect(addr)
            .await
            .with_context(|| format!("connexion à {addr} impossible"))?;
        let name = ServerName::try_from(secure::SERVER_NAME)?;
        let tls = secure::connector()?.connect(name, tcp).await.with_context(|| format!("{addr} ne parle pas TLS"))?;
        let key = secure::session_key(tls.get_ref().1)?;
        exchange(tls, Some((identity, key)), request).await
    };
    let timeout = if matches!(request, Request::Store { .. } | Request::Fetch { .. }) { BLOB_TIMEOUT } else { PEER_TIMEOUT };
    tokio::time::timeout(timeout, attempt)
        .await
        .map_err(|_| anyhow::anyhow!("{addr} ne répond pas"))?
}

/// `peer` : identité éventuelle et clé de session, pour la preuve échangée avant la requête.
async fn exchange<S>(stream: S, peer: Option<(Option<&Identity>, SessionKey)>, request: &Request) -> anyhow::Result<Response>
where
    S: AsyncRead + AsyncWrite,
{
    let (reader, writer) = tokio::io::split(stream);
    let mut writer = Box::pin(writer);

    let mut out = Vec::new();
    if let Some((identity, key)) = &peer {
        out.extend(identity.map(|i| i.handshake("client", key)).unwrap_or_default().into_bytes());
        out.push(b'\n');
    }
    serde_json::to_writer(&mut out, request)?;
    out.push(b'\n');
    writer.write_all(&out).await?;
    writer.flush().await?;

    let mut lines = BufReader::new(reader).lines();
    if let Some((identity, key)) = &peer {
        let sent = lines.next_line().await?.unwrap_or_default();
        anyhow::ensure!(
            identity.is_none_or(|i| i.verify(&sent, "server", key).is_some()),
            "le pair n'est pas membre du cluster"
        );
    }
    let line = lines.next_line().await?.context("le nœud a fermé la connexion sans répondre")?;
    writer.shutdown().await.ok(); // close_notify TLS
    Ok(serde_json::from_str(&line)?)
}

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UnixStream};

use crate::daemon::socket_path;
use crate::protocol::{Request, Response};

const PEER_TIMEOUT: Duration = Duration::from_secs(2);

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

/// Envoie une requête à un nœud distant (`token` : secret du cluster, vide si inconnu), avec un délai maximum.
pub async fn send_to_peer(addr: &str, token: &str, request: &Request) -> anyhow::Result<Response> {
    let attempt = async {
        let stream = TcpStream::connect(addr)
            .await
            .with_context(|| format!("connexion à {addr} impossible"))?;
        exchange(stream, Some(token), request).await
    };
    tokio::time::timeout(PEER_TIMEOUT, attempt)
        .await
        .map_err(|_| anyhow::anyhow!("{addr} ne répond pas"))?
}

async fn exchange<S>(stream: S, token: Option<&str>, request: &Request) -> anyhow::Result<Response>
where
    S: AsyncRead + AsyncWrite,
{
    let (reader, writer) = tokio::io::split(stream);
    let mut writer = Box::pin(writer);

    let mut out = token.map(|t| format!("{t}\n").into_bytes()).unwrap_or_default();
    serde_json::to_writer(&mut out, request)?;
    out.push(b'\n');
    writer.write_all(&out).await?;

    let line = BufReader::new(reader)
        .lines()
        .next_line()
        .await?
        .context("le nœud a fermé la connexion sans répondre")?;
    Ok(serde_json::from_str(&line)?)
}

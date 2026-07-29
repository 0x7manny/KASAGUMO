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
    exchange(stream, request).await
}

/// Envoie une requête à un nœud distant, avec un délai maximum.
pub async fn send_to_peer(addr: &str, request: &Request) -> anyhow::Result<Response> {
    let attempt = async {
        let stream = TcpStream::connect(addr)
            .await
            .with_context(|| format!("connexion à {addr} impossible"))?;
        exchange(stream, request).await
    };
    tokio::time::timeout(PEER_TIMEOUT, attempt)
        .await
        .map_err(|_| anyhow::anyhow!("{addr} ne répond pas"))?
}

async fn exchange<S>(stream: S, request: &Request) -> anyhow::Result<Response>
where
    S: AsyncRead + AsyncWrite,
{
    let (reader, writer) = tokio::io::split(stream);
    let mut writer = Box::pin(writer);

    let mut out = serde_json::to_vec(request)?;
    out.push(b'\n');
    writer.write_all(&out).await?;

    let line = BufReader::new(reader)
        .lines()
        .next_line()
        .await?
        .context("le nœud a fermé la connexion sans répondre")?;
    Ok(serde_json::from_str(&line)?)
}

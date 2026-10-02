use std::path::Path;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::daemon::socket_path;
use crate::protocol::{Request, Response};

pub async fn send(data_dir: &Path, request: &Request) -> anyhow::Result<Response> {
    let path = socket_path(data_dir);
    let stream = UnixStream::connect(&path).await.with_context(|| {
        format!(
            "aucun nœud ne répond sur {} (lance `kgo node start`)",
            path.display()
        )
    })?;
    let (reader, mut writer) = stream.into_split();

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

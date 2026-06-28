use std::path::{Path, PathBuf};

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};

use crate::protocol::{Request, Response};

pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("kgo.sock")
}

pub async fn serve(data_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("impossible de créer {}", data_dir.display()))?;
    let data_dir = std::fs::canonicalize(data_dir)?;
    let path = socket_path(&data_dir);

    if path.exists() {
        if UnixStream::connect(&path).await.is_ok() {
            anyhow::bail!("un nœud tourne déjà dans {}", data_dir.display());
        }
        std::fs::remove_file(&path).context("impossible de supprimer l'ancien socket")?;
    }

    let listener = UnixListener::bind(&path)
        .with_context(|| format!("impossible d'ouvrir le socket {}", path.display()))?;
    println!("nœud démarré, socket : {}", path.display());

    let mut sigterm = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                tokio::spawn(async move {
                    if let Err(e) = handle(stream).await {
                        eprintln!("connexion en erreur : {e:#}");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => break,
            _ = sigterm.recv() => break,
        }
    }

    std::fs::remove_file(&path).ok();
    println!("nœud arrêté");
    Ok(())
}

async fn handle(stream: UnixStream) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => dispatch(request),
            Err(e) => Response::Error(format!("requête invalide : {e}")),
        };
        let mut out = serde_json::to_vec(&response)?;
        out.push(b'\n');
        writer.write_all(&out).await?;
    }
    Ok(())
}

fn dispatch(request: Request) -> Response {
    match request {
        Request::Ps { all: _ } => Response::Workloads(Vec::new()),
    }
}

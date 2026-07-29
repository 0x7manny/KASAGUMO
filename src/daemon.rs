use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinSet;

use crate::client;
use crate::protocol::{NodeInfo, NodeStatus, Request, Response};
use crate::store::Store;
use crate::workload::WorkloadState;

pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("kgo.sock")
}

struct Node {
    store: Store,
    info: NodeInfo,
}

/// Origine d'une connexion : le socket Unix est de confiance, le port TCP
/// (ouvert aux autres machines) ne sert que les requêtes en lecture seule.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Local,
    Remote,
}

pub async fn serve(data_dir: &Path, port: u16) -> anyhow::Result<()> {
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

    let store = Store::open(&data_dir.join("state.redb"))?;

    let tcp = TcpListener::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("impossible d'écouter sur le port {port}"))?;
    let port = tcp.local_addr()?.port();
    let unix = UnixListener::bind(&path)
        .with_context(|| format!("impossible d'ouvrir le socket {}", path.display()))?;

    let info = NodeInfo {
        id: store.node_id()?,
        version: env!("CARGO_PKG_VERSION").to_string(),
        cpus: std::thread::available_parallelism().map_or(1, |n| n.get() as u32),
        port,
    };
    let node = Arc::new(Node { store, info });
    println!("nœud {} démarré", node.info.id);
    println!("socket : {}", path.display());
    println!("tcp : 0.0.0.0:{port}");

    let mut sigterm = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            accepted = unix.accept() => {
                let (stream, _) = accepted?;
                spawn_handler(stream, Arc::clone(&node), Origin::Local);
            }
            accepted = tcp.accept() => {
                let (stream, _) = accepted?;
                spawn_handler(stream, Arc::clone(&node), Origin::Remote);
            }
            _ = tokio::signal::ctrl_c() => break,
            _ = sigterm.recv() => break,
        }
    }

    std::fs::remove_file(&path).ok();
    println!("nœud arrêté");
    Ok(())
}

fn spawn_handler<S>(stream: S, node: Arc<Node>, origin: Origin)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(e) = handle(stream, &node, origin).await {
            eprintln!("connexion en erreur : {e:#}");
        }
    });
}

async fn handle<S>(stream: S, node: &Node, origin: Origin) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite,
{
    let (reader, writer) = tokio::io::split(stream);
    let mut writer = Box::pin(writer);
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => dispatch(request, node, origin).await,
            Err(e) => Response::Error(format!("requête invalide : {e}")),
        };
        let mut out = serde_json::to_vec(&response)?;
        out.push(b'\n');
        writer.write_all(&out).await?;
    }
    Ok(())
}

async fn dispatch(request: Request, node: &Node, origin: Origin) -> Response {
    if origin == Origin::Remote && !matches!(request, Request::Info) {
        return Response::Error("requête refusée depuis le réseau".to_string());
    }
    let store = &node.store;
    let result = match request {
        Request::AddPeer { addr } => store.add_peer(&addr).map(|()| Response::PeerAdded),
        Request::Nodes => nodes(node).await.map(Response::Nodes),
        Request::Info => Ok(Response::Info(node.info.clone())),
        Request::Run(spec) => store.create(spec).map(Response::Started),
        Request::Ps { all } => store.list().map(|mut workloads| {
            if !all {
                workloads.retain(|w| w.state.is_active());
            }
            Response::Workloads(workloads)
        }),
        Request::Stop { id } => store
            .set_state(&id, WorkloadState::Stopped)
            .map(|()| Response::Stopped),
    };
    result.unwrap_or_else(|e| Response::Error(format!("{e:#}")))
}

/// Le nœud local suivi de ses pairs, interrogés en parallèle.
async fn nodes(node: &Node) -> anyhow::Result<Vec<NodeStatus>> {
    let mut queries = JoinSet::new();
    for addr in node.store.peers()? {
        queries.spawn(async move {
            let info = match client::send_to_peer(&addr, &Request::Info).await {
                Ok(Response::Info(info)) => Ok(info),
                Ok(Response::Error(e)) => Err(e),
                Ok(other) => Err(format!("réponse inattendue : {other:?}")),
                Err(e) => Err(format!("{e:#}")),
            };
            NodeStatus { addr, info }
        });
    }

    let mut peers = Vec::new();
    while let Some(status) = queries.join_next().await {
        peers.push(status?);
    }
    peers.sort_by(|a, b| a.addr.cmp(&b.addr));

    let mut statuses = vec![NodeStatus {
        addr: format!("localhost:{}", node.info.port),
        info: Ok(node.info.clone()),
    }];
    statuses.extend(peers);
    Ok(statuses)
}

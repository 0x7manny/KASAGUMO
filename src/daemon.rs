use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinSet;

use crate::client;
use crate::protocol::{NodeInfo, NodeStatus, Request, Response};
use crate::runtime::DockerRuntime;
use crate::store::Store;
use crate::workload::{Workload, WorkloadState};

pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("kgo.sock")
}

struct Node {
    store: Arc<Store>,
    runtime: DockerRuntime,
    info: NodeInfo,
    token: Option<String>,
}

/// Origine d'une connexion : le socket Unix est de confiance ; sur le port TCP,
/// seul un pair qui présente le token du cluster l'est, les autres ne peuvent
/// que demander `Info`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Local,
    Peer,
    Anonymous,
}

pub async fn serve(data_dir: &Path, port: u16, token: Option<String>) -> anyhow::Result<()> {
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
    let node = Arc::new(Node { store: Arc::new(store), runtime: DockerRuntime, info, token });
    reconcile(&node).await?;
    println!("nœud {} démarré", node.info.id);
    println!("socket : {}", path.display());
    println!("tcp : 0.0.0.0:{port}");

    let mut sigterm = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            accepted = unix.accept() => {
                let (stream, _) = accepted?;
                spawn_handler(stream, Arc::clone(&node), true);
            }
            accepted = tcp.accept() => {
                let (stream, _) = accepted?;
                spawn_handler(stream, Arc::clone(&node), false);
            }
            _ = tokio::signal::ctrl_c() => break,
            _ = sigterm.recv() => break,
        }
    }

    std::fs::remove_file(&path).ok();
    println!("nœud arrêté");
    Ok(())
}

fn spawn_handler<S>(stream: S, node: Arc<Node>, local: bool)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(e) = handle(stream, &node, local).await {
            eprintln!("connexion en erreur : {e:#}");
        }
    });
}

async fn handle<S>(stream: S, node: &Node, local: bool) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite,
{
    let (reader, writer) = tokio::io::split(stream);
    let mut writer = Box::pin(writer);
    let mut lines = BufReader::new(reader).lines();

    // sur TCP, la première ligne est le token du cluster
    let origin = if local {
        Origin::Local
    } else {
        let sent = lines.next_line().await?.unwrap_or_default();
        match node.token.as_deref() {
            Some(token) if token == sent => Origin::Peer,
            _ => Origin::Anonymous,
        }
    };

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
    let allowed = match origin {
        Origin::Local => true,
        Origin::Peer => !matches!(request, Request::Forward { .. }),
        Origin::Anonymous => matches!(request, Request::Info),
    };
    if !allowed {
        return Response::Error("requête refusée : token du cluster manquant ou invalide".to_string());
    }
    let store = &node.store;
    let result = match request {
        Request::AddPeer { addr } => store.add_peer(&addr).map(|()| Response::PeerAdded),
        Request::Nodes => nodes(node).await.map(Response::Nodes),
        Request::Info => Ok(Response::Info(node.info.clone())),
        Request::Run(spec) => store.create(spec).map(|workload| {
            tokio::spawn(launch(Arc::clone(store), workload.clone()));
            Response::Started(workload)
        }),
        Request::Ps { all } => store.list().map(|mut workloads| {
            if !all {
                workloads.retain(|w| w.state.is_active());
            }
            Response::Workloads(workloads)
        }),
        Request::Stop { id } => stop(node, &id).await.map(|()| Response::Stopped),
        Request::Forward { addr, request } => {
            client::send_to_peer(&addr, node.token.as_deref().unwrap_or_default(), &request).await
        }
    };
    result.unwrap_or_else(|e| Response::Error(format!("{e:#}")))
}

/// Le nœud local suivi de ses pairs, interrogés en parallèle.
async fn nodes(node: &Node) -> anyhow::Result<Vec<NodeStatus>> {
    let mut queries = JoinSet::new();
    for addr in node.store.peers()? {
        let token = node.token.clone().unwrap_or_default();
        queries.spawn(async move {
            let info = match client::send_to_peer(&addr, &token, &Request::Info).await {
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

/// Fait avancer un workload : Pending → Pulling → Running, ou Failed si Docker échoue.
async fn launch(store: Arc<Store>, workload: Workload) {
    use WorkloadState::*;
    let outcome = async {
        if !store.advance(&workload.id, Pending, Pulling)? {
            return Ok(());
        }
        DockerRuntime.pull(&workload.spec.image).await?;
        DockerRuntime.start(&workload).await?;
        if !store.advance(&workload.id, Pulling, Running)? {
            // arrêté pendant le démarrage : le conteneur vient d'être créé, on le retire
            DockerRuntime.stop(&workload.id).await?;
        }
        anyhow::Ok(())
    }
    .await;
    if let Err(e) = outcome {
        eprintln!("workload {} en échec : {e:#}", workload.id);
        store.advance(&workload.id, Pulling, Failed).ok();
    }
}

async fn stop(node: &Node, id: &str) -> anyhow::Result<()> {
    // set_state échoue si le workload est inconnu : on ne touche pas à Docker dans ce cas
    node.store.set_state(id, WorkloadState::Stopped)?;
    if let Err(e) = node.runtime.stop(id).await {
        eprintln!("arrêt du conteneur de {id} : {e:#}");
    }
    Ok(())
}

/// Remet la base en accord avec la réalité après un redémarrage du daemon :
/// un démarrage interrompu ou un conteneur disparu ne sont plus « actifs ».
async fn reconcile(node: &Node) -> anyhow::Result<()> {
    use WorkloadState::*;
    for w in node.store.list()? {
        let alive = match w.state {
            Pending | Pulling => false,
            Running => node.runtime.is_running(&w.id).await,
            Stopped | Failed => continue,
        };
        if !alive {
            eprintln!("workload {} ({}) : {} → failed", w.id, w.spec.image, w.state);
            node.store.advance(&w.id, w.state, Failed)?;
        }
    }
    Ok(())
}

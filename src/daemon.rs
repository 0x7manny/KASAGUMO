use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

use crate::client;
use crate::protocol::{NodeInfo, NodeStatus, Request, Response};
use crate::runtime::DockerRuntime;
use crate::secure::{self, SessionKey};
use crate::store::Store;
use crate::workload::{Placement, Workload, WorkloadSpec, WorkloadState};

pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("kgo.sock")
}

struct Node {
    store: Arc<Store>,
    runtime: DockerRuntime,
    info: NodeInfo,
    token: Option<String>,
    tls: TlsAcceptor,
}

/// Origine d'une connexion : le socket Unix est de confiance ; sur le port TCP,
/// seul un pair qui présente le token du cluster l'est, les autres ne peuvent
/// que demander `Info`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Local,
    Peer(IpAddr),
    Anonymous,
}

pub async fn serve(data_dir: &Path, port: u16, token: Option<String>, heartbeat: Duration) -> anyhow::Result<()> {
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
        used_cpus: 0,
        used_memory: 0,
    };
    let node = Arc::new(Node { store: Arc::new(store), runtime: DockerRuntime, info, token, tls: secure::acceptor()? });
    reconcile(&node).await?;
    tokio::spawn(monitor(Arc::clone(&node), heartbeat));
    println!("nœud {} démarré", node.info.id);
    println!("socket : {}", path.display());
    println!("tcp : 0.0.0.0:{port}");

    let mut sigterm = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            accepted = unix.accept() => {
                let (stream, _) = accepted?;
                let node = Arc::clone(&node);
                spawn_handler(async move { handle(stream, &node, None).await });
            }
            accepted = tcp.accept() => {
                let (stream, from) = accepted?;
                let node = Arc::clone(&node);
                spawn_handler(async move {
                    let tls = node.tls.accept(stream).await?;
                    let key = secure::session_key(tls.get_ref().1)?;
                    handle(tls, &node, Some((key, from.ip()))).await
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

fn spawn_handler(connection: impl Future<Output = anyhow::Result<()>> + Send + 'static) {
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connexion en erreur : {e:#}");
        }
    });
}

/// `link` : clé de session TLS et adresse du pair, absentes sur le socket Unix.
async fn handle<S>(stream: S, node: &Node, link: Option<(SessionKey, IpAddr)>) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite,
{
    let (reader, writer) = tokio::io::split(stream);
    let mut writer = Box::pin(writer);
    let mut lines = BufReader::new(reader).lines();

    // sur TCP, le pair ouvre par sa preuve du token, à quoi le nœud répond par la sienne
    let origin = match link {
        None => Origin::Local,
        Some((key, ip)) => {
            let sent = lines.next_line().await?.unwrap_or_default();
            let token = node.token.as_deref().unwrap_or_default();
            writer.write_all(format!("{}\n", secure::proof(token, &key, "server")).as_bytes()).await?;
            if !token.is_empty() && secure::same(&sent, &secure::proof(token, &key, "client")) {
                Origin::Peer(ip)
            } else {
                Origin::Anonymous
            }
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
        Origin::Peer(_) => !matches!(request, Request::Forward { .. } | Request::Schedule(_)),
        Origin::Anonymous => matches!(request, Request::Info),
    };
    if !allowed {
        return Response::Error("requête refusée : token du cluster manquant ou invalide".to_string());
    }
    let store = &node.store;
    let result = match request {
        Request::AddPeer { addr } => store.add_peer(&addr).map(|()| Response::PeerAdded),
        Request::Nodes => nodes(node).await.map(Response::Nodes),
        Request::Info => node.current_info().map(Response::Info),
        Request::Run(spec) => run(node, spec).map(Response::Started),
        Request::Schedule(spec) => schedule(node, spec).await,
        Request::Ps { all } => store.list().map(|mut workloads| {
            if !all {
                workloads.retain(|w| w.state.is_active());
            }
            Response::Workloads(workloads)
        }),
        Request::Stop { id } => stop(node, &id).await.map(|()| Response::Stopped),
        Request::Hello { port } => match origin {
            Origin::Peer(ip) => store
                .add_peer(&SocketAddr::new(ip, port).to_string())
                .and_then(|()| store.peers())
                .map(Response::Peers),
            _ => Err(anyhow::anyhow!("réservé aux pairs")),
        },
        Request::Forward { addr, request } => forward(node, &addr, &request).await,
    };
    result.unwrap_or_else(|e| Response::Error(format!("{e:#}")))
}

impl Node {
    /// Les infos du nœud, avec les ressources réservées à cet instant.
    fn current_info(&self) -> anyhow::Result<NodeInfo> {
        let mut info = self.info.clone();
        for w in self.store.list()?.iter().filter(|w| w.state.is_active()) {
            info.used_cpus += w.spec.cpu;
            info.used_memory += w.spec.memory;
        }
        Ok(info)
    }
}

fn run(node: &Node, spec: WorkloadSpec) -> anyhow::Result<Workload> {
    let free = node.current_info()?.free_cpus();
    anyhow::ensure!(spec.cpu <= free, "{} CPU demandés, {free} libres", spec.cpu);
    let workload = node.store.create(spec)?;
    tokio::spawn(launch(Arc::clone(&node.store), workload.clone()));
    Ok(workload)
}

/// Choisit le nœud qui a le plus de CPU libres (le local à égalité) et y lance le workload.
async fn schedule(node: &Node, spec: WorkloadSpec) -> anyhow::Result<Response> {
    let (addr, target) = nodes(node)
        .await?
        .into_iter()
        .rev()
        .filter_map(|n| Some((n.addr, n.info.ok()?)))
        .filter(|(_, info)| info.free_cpus() >= spec.cpu)
        .max_by_key(|(_, info)| info.free_cpus())
        .with_context(|| format!("aucun nœud n'a {} CPU libres", spec.cpu))?;

    if target.id == node.info.id {
        return Ok(Response::Placed { addr, workload: run(node, spec)? });
    }
    match forward(node, &addr, &Request::Run(spec)).await? {
        Response::Started(workload) => {
            let placement = Placement { addr: addr.clone(), spec: workload.spec.clone() };
            node.store.place(&workload.id, &placement)?;
            Ok(Response::Placed { addr, workload })
        }
        other => Ok(other),
    }
}

async fn forward(node: &Node, addr: &str, request: &Request) -> anyhow::Result<Response> {
    let response = client::send_to_peer(addr, node.token.as_deref().unwrap_or_default(), request).await?;
    if let (Request::Stop { id }, Response::Stopped) = (request, &response) {
        node.store.unplace(id)?; // arrêté volontairement : à ne pas relancer ailleurs
    }
    Ok(response)
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
        info: node.current_info().map_err(|e| format!("{e:#}")),
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

/// Battements de cœur : les pairs apprennent notre adresse et se transmettent leurs pairs ;
/// un pair muet depuis `DEAD_AFTER` battements est déclaré tombé et ses workloads sont replacés.
const DEAD_AFTER: u32 = 3;

async fn monitor(node: Arc<Node>, interval: Duration) {
    let mut misses = HashMap::new();
    let mut own = HashSet::new();
    loop {
        tokio::time::sleep(interval).await;
        if let Err(e) = heartbeat(&node, &mut misses, &mut own).await {
            eprintln!("battement de cœur en erreur : {e:#}");
        }
    }
}

/// `misses` : battements manqués par pair ; `own` : adresses qui pointent vers ce nœud.
async fn heartbeat(
    node: &Node,
    misses: &mut HashMap<String, u32>,
    own: &mut HashSet<String>,
) -> anyhow::Result<()> {
    let token = node.token.clone().unwrap_or_default();
    let mut probes = JoinSet::new();
    for addr in node.store.peers()?.into_iter().filter(|a| !own.contains(a)) {
        let token = token.clone();
        let hello = Request::Hello { port: node.info.port };
        probes.spawn(async move { (client::send_to_peer(&addr, &token, &hello).await, addr) });
    }
    while let Some(probe) = probes.join_next().await {
        match probe? {
            (Ok(Response::Peers(known)), addr) => {
                misses.remove(&addr);
                for candidate in known {
                    learn(node, candidate, own).await?;
                }
            }
            (_, addr) => *misses.entry(addr).or_default() += 1,
        }
    }
    for addr in misses.iter().filter(|(_, n)| **n >= DEAD_AFTER).map(|(a, _)| a) {
        failover(node, addr).await?;
    }
    Ok(())
}

/// Retient un pair appris par un autre, sauf si l'adresse est la nôtre.
async fn learn(node: &Node, addr: String, own: &mut HashSet<String>) -> anyhow::Result<()> {
    if own.contains(&addr) || node.store.peers()?.contains(&addr) {
        return Ok(());
    }
    let token = node.token.as_deref().unwrap_or_default();
    match client::send_to_peer(&addr, token, &Request::Info).await {
        Ok(Response::Info(info)) if info.id == node.info.id => {
            own.insert(addr);
        }
        Ok(Response::Info(_)) => node.store.add_peer(&addr)?,
        _ => {}
    }
    Ok(())
}

/// Replace ailleurs les workloads confiés à un pair tombé ; réessayé tant qu'il en reste.
async fn failover(node: &Node, dead: &str) -> anyhow::Result<()> {
    for (id, placement) in node.store.placements()?.into_iter().filter(|(_, p)| p.addr == dead) {
        match schedule(node, placement.spec).await? {
            Response::Placed { addr, workload } => {
                eprintln!("workload {id} : {dead} est tombé, relancé sur {addr} ({})", workload.id);
                node.store.unplace(&id)?;
            }
            other => eprintln!("workload {id} : replacement impossible ({other:?})"),
        }
    }
    Ok(())
}

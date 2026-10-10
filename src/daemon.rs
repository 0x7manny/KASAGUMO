use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

use crate::blobs::{self, Blobs};
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
    blobs: Blobs,
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
        memory: total_memory().await.unwrap_or(u64::MAX), // inconnue : pas de limite
        port,
        used_cpus: 0,
        used_memory: 0,
    };
    let node = Arc::new(Node { store: Arc::new(store), runtime: DockerRuntime, info, token, tls: secure::acceptor()?, blobs: Blobs::open(data_dir.join("blobs"))? });
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
            writer.flush().await?;
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
        writer.flush().await?; // sans flush, la fin d'un gros message peut rester dans le tampon TLS
    }
    Ok(())
}

async fn dispatch(request: Request, node: &Node, origin: Origin) -> Response {
    let allowed = match origin {
        Origin::Local => true,
        Origin::Peer(_) => {
            !matches!(request, Request::Forward { .. } | Request::Schedule(_) | Request::Put { .. } | Request::Get { .. })
        }
        Origin::Anonymous => matches!(request, Request::Info),
    };
    if !allowed {
        return Response::Error("requête refusée : token du cluster manquant ou invalide".to_string());
    }
    let store = &node.store;
    // un workload confié à un pair se pilote là où il tourne
    if let Request::Stop { id } | Request::Logs { id } = &request
        && let Ok(Some(placement)) = store.placement(id) {
            let response = forward(node, &placement.addr, &request).await;
            return response.unwrap_or_else(|e| Response::Error(format!("{e:#}")));
        }
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
        Request::Logs { id } => logs(node, &id).await.map(Response::Logs),
        Request::Store { data } => blobs::decode(&data)
            .and_then(|data| node.blobs.put(&data))
            .map(|id| Response::Stored { id, copies: 1 }),
        Request::Missing { ids } => ids
            .into_iter()
            .filter_map(|id| node.blobs.get(&id).map(|blob| blob.is_none().then_some(id)).transpose())
            .collect::<anyhow::Result<_>>()
            .map(Response::Ids),
        Request::Fetch { id } => node.blobs.get(&id).map(|data| Response::Blob(data.map(|d| blobs::encode(&d)))),
        Request::Put { data } => put(node, &data).await,
        Request::Get { id } => get(node, &id).await,
        Request::Hello { port, workloads } => match origin {
            Origin::Peer(ip) => {
                let addr = SocketAddr::new(ip, port).to_string();
                store.add_peer(&addr).and_then(|()| store.set_hosted(&addr, workloads)).and_then(|()| store.peers()).map(Response::Peers)
            }
            _ => Err(anyhow::anyhow!("réservé aux pairs")),
        },
        Request::Adopt { dead, workloads } => takeover(node, &dead, workloads).await.map(|()| Response::Stopped),
        Request::Forget { ids } => ids.iter().try_for_each(|id| store.unplace(id)).map(|()| Response::Stopped),
        Request::Forward { addr, request } => forward(node, &addr, &request).await,
    };
    result.unwrap_or_else(|e| Response::Error(format!("{e:#}")))
}

impl Node {
    /// Les workloads actifs de ce nœud, tels qu'on les annonce aux pairs.
    fn hosted(&self) -> anyhow::Result<Vec<(String, WorkloadSpec)>> {
        let workloads = self.store.list()?;
        Ok(workloads.into_iter().filter(|w| w.state.is_active()).map(|w| (w.id, w.spec)).collect())
    }

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
    let info = node.current_info()?;
    anyhow::ensure!(
        info.fits(&spec),
        "ressources insuffisantes : {} CPU et {} Mo demandés, {} CPU et {} Mo libres",
        spec.cpu,
        spec.memory >> 20,
        info.free_cpus(),
        info.free_memory() >> 20
    );
    let workload = node.store.create(spec)?;
    tokio::spawn(launch(Arc::clone(&node.store), workload.clone()));
    Ok(workload)
}

/// Choisit le nœud qui a le plus de CPU puis de mémoire libres (le local à égalité) et y lance le workload.
async fn schedule(node: &Node, spec: WorkloadSpec) -> anyhow::Result<Response> {
    let (addr, target) = nodes(node)
        .await?
        .into_iter()
        .rev()
        .filter_map(|n| Some((n.addr, n.info.ok()?)))
        .filter(|(_, info)| info.fits(&spec))
        .max_by_key(|(_, info)| (info.free_cpus(), info.free_memory()))
        .with_context(|| format!("aucun nœud n'a {} CPU et {} Mo libres", spec.cpu, spec.memory >> 20))?;

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

async fn logs(node: &Node, id: &str) -> anyhow::Result<String> {
    anyhow::ensure!(node.store.list()?.iter().any(|w| w.id == id), "workload introuvable : {id}");
    node.runtime.logs(id).await
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
/// La réparation des blocs passe tous les `REPAIR_EVERY` battements.
const REPAIR_EVERY: u32 = 10;

async fn monitor(node: Arc<Node>, interval: Duration) {
    let mut misses = HashMap::new();
    let mut own = HashSet::new();
    for tick in 1u32.. {
        tokio::time::sleep(interval).await;
        if let Err(e) = heartbeat(&node, &mut misses, &mut own).await {
            eprintln!("battement de cœur en erreur : {e:#}");
        }
        if tick % REPAIR_EVERY == 0
            && let Err(e) = repair(&node).await {
                eprintln!("réparation des blocs en erreur : {e:#}");
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
        let hello = Request::Hello { port: node.info.port, workloads: node.hosted()? };
        probes.spawn(async move { (client::send_to_peer(&addr, &token, &hello).await, addr) });
    }
    while let Some(probe) = probes.join_next().await {
        match probe? {
            (Ok(Response::Peers(known)), addr) => {
                misses.remove(&addr);
                release_orphans(node, &addr).await?;
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

/// Un pair est tombé : ses workloads (lancés par lui ou par un autre) sont relancés ailleurs par le nœud
/// vivant au plus petit id. Les autres lui transmettent ce qu'ils en savent ; réessayé tant qu'il en reste.
async fn failover(node: &Node, dead: &str) -> anyhow::Result<()> {
    let lost: Vec<_> = node.store.placements()?.into_iter().filter(|(_, p)| p.addr == dead).map(|(id, p)| (id, p.spec)).collect();
    if lost.is_empty() {
        return Ok(());
    }
    let alive = nodes(node).await?.into_iter().filter_map(|n| Some((n.addr, n.info.ok()?)));
    let Some((leader_addr, leader)) = alive.min_by(|a, b| a.1.id.cmp(&b.1.id)) else {
        return Ok(());
    };
    if leader.id == node.info.id {
        return takeover(node, dead, lost).await;
    }
    let token = node.token.as_deref().unwrap_or_default();
    let adopt = Request::Adopt { dead: dead.to_string(), workloads: lost };
    client::send_to_peer(&leader_addr, token, &adopt).await.ok();
    Ok(())
}

/// Relance `lost` (workloads de `dead`) puis prévient les pairs ; ceux déjà relancés ne le sont pas deux fois.
async fn takeover(node: &Node, dead: &str, lost: Vec<(String, WorkloadSpec)>) -> anyhow::Result<()> {
    let done: HashSet<_> = node.store.orphans()?.into_iter().map(|(id, _)| id).collect();
    let mut moved = Vec::new();
    for (id, spec) in lost {
        if !done.contains(&id) {
            match schedule(node, spec).await? {
                Response::Placed { addr, workload } => {
                    eprintln!("workload {id} : {dead} est tombé, relancé sur {addr} ({})", workload.id);
                    node.store.orphan(&id, dead)?;
                }
                other => {
                    eprintln!("workload {id} : replacement impossible ({other:?})");
                    continue;
                }
            }
        }
        node.store.unplace(&id)?;
        moved.push(id);
    }

    let token = node.token.as_deref().unwrap_or_default();
    for peer in node.store.peers()? {
        client::send_to_peer(&peer, token, &Request::Forget { ids: moved.clone() }).await.ok();
    }
    Ok(())
}

/// Un pair revenu d'entre les morts : arrête les copies de workloads qu'on a relancés ailleurs.
async fn release_orphans(node: &Node, addr: &str) -> anyhow::Result<()> {
    let token = node.token.as_deref().unwrap_or_default();
    for (id, _) in node.store.orphans()?.into_iter().filter(|(_, at)| at == addr) {
        if client::send_to_peer(addr, token, &Request::Stop { id: id.clone() }).await.is_ok() {
            node.store.forget_orphan(&id)?;
        }
    }
    Ok(())
}

/// Mémoire physique de la machine, via `/proc/meminfo` (Linux) ou `sysctl` (macOS).
async fn total_memory() -> Option<u64> {
    if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
        let kib = meminfo.lines().find_map(|l| l.strip_prefix("MemTotal:")?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok());
        return kib.map(|kib| kib << 10);
    }
    let out = tokio::process::Command::new("sysctl").args(["-n", "hw.memsize"]).output().await;
    out.ok().and_then(|o| String::from_utf8(o.stdout).ok()?.trim().parse().ok())
}

/// Nombre de copies de chaque bloc.
const REPLICAS: usize = 2;

/// Les nœuds joignables, classés par le hachage (id du bloc, id du nœud) : les `REPLICAS` premiers
/// doivent détenir le bloc.
async fn ranked_nodes(node: &Node, id: &str) -> anyhow::Result<Vec<(String, NodeInfo)>> {
    let mut ranked: Vec<_> = nodes(node).await?.into_iter().filter_map(|n| Some((n.addr, n.info.ok()?))).collect();
    ranked.sort_by_cached_key(|(_, info)| Sha256::digest(format!("{id}{}", info.id)));
    Ok(ranked)
}

/// Range le bloc sur les `REPLICAS` nœuds que le hachage (id du bloc, id du nœud) classe en tête ;
/// un nœud injoignable est remplacé par le suivant du classement.
async fn put(node: &Node, hex: &str) -> anyhow::Result<Response> {
    let data = blobs::decode(hex)?;
    let id = kasagumo::ChunkId::of(&data).to_string();
    let ranked = ranked_nodes(node, &id).await?;

    let token = node.token.as_deref().unwrap_or_default();
    let mut copies = 0;
    for (addr, info) in ranked {
        if copies == REPLICAS {
            break;
        }
        let stored = if info.id == node.info.id {
            node.blobs.put(&data).is_ok()
        } else {
            let store = Request::Store { data: hex.to_string() };
            matches!(client::send_to_peer(&addr, token, &store).await, Ok(Response::Stored { .. }))
        };
        copies += usize::from(stored);
    }
    anyhow::ensure!(copies > 0, "aucun nœud n'a pu stocker le bloc {id}");
    Ok(Response::Stored { id, copies })
}

/// Cherche le bloc ici puis chez les pairs ; son contenu est revérifié contre son id.
async fn get(node: &Node, id: &str) -> anyhow::Result<Response> {
    if let Some(data) = node.blobs.get(id)? {
        return Ok(Response::Blob(Some(blobs::encode(&data))));
    }
    let token = node.token.as_deref().unwrap_or_default();
    for addr in node.store.peers()? {
        let fetch = Request::Fetch { id: id.to_string() };
        if let Ok(Response::Blob(Some(hex))) = client::send_to_peer(&addr, token, &fetch).await
            && blobs::decode(&hex).is_ok_and(|data| kasagumo::ChunkId::of(&data).to_string() == id) {
                return Ok(Response::Blob(Some(hex)));
            }
    }
    anyhow::bail!("bloc introuvable dans le cluster : {id}")
}

/// Remet chaque bloc local sur les nœuds qui doivent le détenir (après la chute d'un nœud ou l'arrivée d'un autre).
async fn repair(node: &Node) -> anyhow::Result<()> {
    let mut wanted: HashMap<String, Vec<String>> = HashMap::new();
    for id in node.blobs.ids()? {
        for (addr, info) in ranked_nodes(node, &id).await?.into_iter().take(REPLICAS) {
            if info.id != node.info.id {
                wanted.entry(addr).or_default().push(id.clone());
            }
        }
    }

    let token = node.token.as_deref().unwrap_or_default();
    for (addr, ids) in wanted {
        let Ok(Response::Ids(missing)) = client::send_to_peer(&addr, token, &Request::Missing { ids }).await else {
            continue;
        };
        for id in missing {
            if let Some(data) = node.blobs.get(&id)? {
                let store = Request::Store { data: blobs::encode(&data) };
                client::send_to_peer(&addr, token, &store).await.ok();
            }
        }
    }
    Ok(())
}

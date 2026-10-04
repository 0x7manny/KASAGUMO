use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use kasagumo::{FileChunk, FileManifest, FilePrimitive};

use crate::cli::{NodeAction, RunArgs};
use crate::blobs;
use crate::protocol::{Request, Response};
use crate::workload::WorkloadSpec;
use crate::{client, daemon};

pub async fn node(action: NodeAction, data_dir: &Path) -> anyhow::Result<()> {
    match action {
        NodeAction::Start { port, token, heartbeat_ms } => daemon::serve(data_dir, port, token, Duration::from_millis(heartbeat_ms)).await,
        NodeAction::Join { addr } => join(&addr, data_dir).await,
    }
}

async fn join(addr: &str, data_dir: &Path) -> anyhow::Result<()> {
    // le pair doit répondre avant d'être retenu
    match client::send_to_peer(addr, "", &Request::Info).await? {
        Response::Info(info) => {
            match client::send(data_dir, &Request::AddPeer { addr: addr.to_string() }).await? {
                Response::PeerAdded => println!("pair ajouté : {addr} (nœud {})", info.id),
                Response::Error(e) => anyhow::bail!("{e}"),
                other => anyhow::bail!("réponse inattendue : {other:?}"),
            }
        }
        Response::Error(e) => anyhow::bail!("{addr} : {e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
    Ok(())
}

pub async fn nodes(data_dir: &Path) -> anyhow::Result<()> {
    match client::send(data_dir, &Request::Nodes).await? {
        Response::Nodes(nodes) => {
            println!("{:<24} {:<18} {:<7} {:<10} {:<8} STATUT", "ADRESSE", "ID", "CPU", "MÉMOIRE", "VERSION");
            for n in nodes {
                match n.info {
                    Ok(i) => {
                        let (cpu, memory) = (format!("{}/{}", i.free_cpus(), i.cpus), format!("{}/{}Mo", i.free_memory() >> 20, i.memory >> 20));
                        println!("{:<24} {:<18} {:<7} {:<10} {:<8} up", n.addr, i.id, cpu, memory, i.version)
                    }
                    Err(e) => println!("{:<24} {:<18} {:<7} {:<10} {:<8} injoignable ({e})", n.addr, "-", "-", "-", "-"),
                }
            }
        }
        Response::Error(e) => anyhow::bail!("{e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
    Ok(())
}

pub async fn run(args: RunArgs, on: Option<&str>, data_dir: &Path) -> anyhow::Result<()> {
    let spec = WorkloadSpec {
        image: args.image,
        cpu: args.cpu,
        memory: args.memory,
    };
    let request = if on.is_some() { Request::Run(spec) } else { Request::Schedule(spec) };
    match client::send_on(data_dir, on, request).await? {
        Response::Placed { addr, workload: w } => println!("{} ({}) : {} sur {addr}", w.id, w.spec.image, w.state),
        Response::Started(w) => println!("{} ({}) : {}", w.id, w.spec.image, w.state),
        Response::Error(e) => anyhow::bail!("{e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
    Ok(())
}

pub async fn ps(all: bool, on: Option<&str>, data_dir: &Path) -> anyhow::Result<()> {
    match client::send_on(data_dir, on, Request::Ps { all }).await? {
        Response::Workloads(workloads) if workloads.is_empty() => println!("aucun workload"),
        Response::Workloads(workloads) => {
            println!("{:<10} {:<24} {:<5} {:<8} STATUT", "ID", "IMAGE", "CPU", "MÉMOIRE");
            for w in workloads {
                println!(
                    "{:<10} {:<24} {:<5} {:<8} {}",
                    w.id,
                    w.spec.image,
                    w.spec.cpu,
                    format!("{}MB", w.spec.memory >> 20),
                    w.state
                );
            }
        }
        Response::Error(e) => anyhow::bail!("{e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
    Ok(())
}

pub async fn stop(workload_id: &str, on: Option<&str>, data_dir: &Path) -> anyhow::Result<()> {
    let request = Request::Stop { id: workload_id.to_string() };
    match client::send_on(data_dir, on, request).await? {
        Response::Stopped => println!("{workload_id} : stopped"),
        Response::Error(e) => anyhow::bail!("{e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
    Ok(())
}

pub async fn logs(workload_id: &str, on: Option<&str>, data_dir: &Path) -> anyhow::Result<()> {
    let request = Request::Logs { id: workload_id.to_string() };
    match client::send_on(data_dir, on, request).await? {
        Response::Logs(output) => print!("{output}"),
        Response::Error(e) => anyhow::bail!("{e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
    Ok(())
}

/// Envoie un bloc au nœud local, qui le répartit ; renvoie son id et le nombre de copies.
async fn put_blob(data: &[u8], data_dir: &Path) -> anyhow::Result<(String, usize)> {
    match client::send(data_dir, &Request::Put { data: blobs::encode(data) }).await? {
        Response::Stored { id, copies } => Ok((id, copies)),
        Response::Error(e) => anyhow::bail!("{e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
}

async fn get_blob(id: &str, data_dir: &Path) -> anyhow::Result<Vec<u8>> {
    match client::send(data_dir, &Request::Get { id: id.to_string() }).await? {
        Response::Blob(Some(hex)) => blobs::decode(&hex),
        Response::Error(e) => anyhow::bail!("{e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
}

pub async fn put(path: &Path, data_dir: &Path) -> anyhow::Result<()> {
    let file = FilePrimitive::from_path(path).with_context(|| format!("impossible de lire {}", path.display()))?;
    let mut copies = usize::MAX;
    for chunk in file.chunks() {
        copies = copies.min(put_blob(&chunk.data, data_dir).await?.1);
    }
    // le manifeste est un bloc comme les autres : son id identifie le fichier
    let (id, manifest_copies) = put_blob(&serde_json::to_vec(file.manifest())?, data_dir).await?;
    println!("{id}");
    eprintln!("{} : {} blocs, {} copies minimum", file.name(), file.chunks().len(), copies.min(manifest_copies));
    Ok(())
}

pub async fn get(id: &str, out: &Path, data_dir: &Path) -> anyhow::Result<()> {
    let manifest: FileManifest = serde_json::from_slice(&get_blob(id, data_dir).await?)
        .context("cet identifiant n'est pas celui d'un fichier")?;
    let mut chunks = Vec::new();
    for (index, chunk_id) in manifest.chunks.iter().enumerate() {
        let data = get_blob(&chunk_id.to_string(), data_dir).await?;
        chunks.push(FileChunk { id: *chunk_id, index: index as u64, data });
    }
    FilePrimitive::from_parts(manifest, chunks)?.write_to_path(out)?;
    println!("{} écrit", out.display());
    Ok(())
}

pub fn chunk(path: &Path) -> anyhow::Result<()> {
    let file = FilePrimitive::from_path(path)
        .with_context(|| format!("impossible de découper {}", path.display()))?;

    println!("{file}");
    for chunk in file.chunks() {
        println!("  {chunk}");
    }

    match file.verify() {
        Ok(()) => println!("intégrité : OK"),
        Err(e) => anyhow::bail!("intégrité : ÉCHEC ({e})"),
    }
    Ok(())
}

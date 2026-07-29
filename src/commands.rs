use std::path::Path;

use anyhow::Context;
use kasagumo::FilePrimitive;

use crate::cli::{NodeAction, RunArgs};
use crate::protocol::{Request, Response};
use crate::workload::WorkloadSpec;
use crate::{client, daemon};

pub async fn node(action: NodeAction, data_dir: &Path) -> anyhow::Result<()> {
    match action {
        NodeAction::Start { port } => daemon::serve(data_dir, port).await,
        NodeAction::Join { addr } => join(&addr, data_dir).await,
    }
}

async fn join(addr: &str, data_dir: &Path) -> anyhow::Result<()> {
    // le pair doit répondre avant d'être retenu
    match client::send_to_peer(addr, &Request::Info).await? {
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
            println!("{:<24} {:<18} {:<5} {:<8} STATUT", "ADRESSE", "ID", "CPU", "VERSION");
            for n in nodes {
                match n.info {
                    Ok(i) => println!("{:<24} {:<18} {:<5} {:<8} up", n.addr, i.id, i.cpus, i.version),
                    Err(e) => println!("{:<24} {:<18} {:<5} {:<8} injoignable ({e})", n.addr, "-", "-", "-"),
                }
            }
        }
        Response::Error(e) => anyhow::bail!("{e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
    Ok(())
}

pub async fn run(args: RunArgs, data_dir: &Path) -> anyhow::Result<()> {
    let spec = WorkloadSpec {
        image: args.image,
        cpu: args.cpu,
        memory: args.memory,
    };
    match client::send(data_dir, &Request::Run(spec)).await? {
        Response::Started(w) => println!("{} ({}) : {}", w.id, w.spec.image, w.state),
        Response::Error(e) => anyhow::bail!("{e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
    Ok(())
}

pub async fn ps(all: bool, data_dir: &Path) -> anyhow::Result<()> {
    match client::send(data_dir, &Request::Ps { all }).await? {
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

pub async fn stop(workload_id: &str, data_dir: &Path) -> anyhow::Result<()> {
    let request = Request::Stop {
        id: workload_id.to_string(),
    };
    match client::send(data_dir, &request).await? {
        Response::Stopped => println!("{workload_id} : stopped"),
        Response::Error(e) => anyhow::bail!("{e}"),
        other => anyhow::bail!("réponse inattendue : {other:?}"),
    }
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

use std::path::Path;

use anyhow::Context;
use kasagumo::FilePrimitive;

use crate::cli::{NodeAction, RunArgs};
use crate::protocol::{Request, Response};
use crate::workload::WorkloadSpec;
use crate::{client, daemon};

fn not_yet(command: &str) -> anyhow::Result<()> {
    anyhow::bail!("`kgo {command}` n'est pas encore implémenté (prévu en M1)")
}

pub async fn node(action: NodeAction, data_dir: &Path) -> anyhow::Result<()> {
    match action {
        NodeAction::Start { port: _ } => daemon::serve(data_dir).await,
    }
}

pub fn nodes() -> anyhow::Result<()> {
    not_yet("nodes")
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

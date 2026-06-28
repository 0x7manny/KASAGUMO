use std::path::Path;

use anyhow::Context;
use kasagumo::FilePrimitive;

use crate::cli::{NodeAction, RunArgs};
use crate::protocol::{Request, Response};
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

pub fn run(args: RunArgs) -> anyhow::Result<()> {
    println!(
        "workload demandé : {} ({} CPU, {} Mo)",
        args.image,
        args.cpu,
        args.memory >> 20
    );
    not_yet("run")
}

pub async fn ps(all: bool, data_dir: &Path) -> anyhow::Result<()> {
    match client::send(data_dir, &Request::Ps { all }).await? {
        Response::Workloads(workloads) if workloads.is_empty() => println!("aucun workload"),
        Response::Workloads(workloads) => {
            println!("{:<14} {:<24} STATUT", "ID", "IMAGE");
            for w in workloads {
                println!("{:<14} {:<24} {}", w.id, w.image, w.status);
            }
        }
        Response::Error(e) => anyhow::bail!("{e}"),
    }
    Ok(())
}

pub fn stop(workload_id: &str) -> anyhow::Result<()> {
    println!("arrêt demandé : {workload_id}");
    not_yet("stop")
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

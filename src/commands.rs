use std::path::Path;

use anyhow::Context;
use kasagumo::FilePrimitive;

use crate::cli::{NodeAction, RunArgs};

fn not_yet(command: &str) -> anyhow::Result<()> {
    anyhow::bail!("`kgo {command}` n'est pas encore implémenté (prévu en M1)")
}

pub fn node(action: NodeAction) -> anyhow::Result<()> {
    match action {
        NodeAction::Start { port, data_dir } => {
            println!("nœud demandé : port {port}, données dans {}", data_dir.display());
            not_yet("node start")
        }
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

pub fn ps(_all: bool) -> anyhow::Result<()> {
    not_yet("ps")
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

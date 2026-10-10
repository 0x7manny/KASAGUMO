mod blobs;
mod cli;
mod client;
mod commands;
mod daemon;
mod identity;
mod protocol;
mod runtime;
mod secure;
mod store;
mod workload;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Node { action } => commands::node(action, &cli.data_dir).await,
        Command::Cluster { action } => commands::cluster(action, &cli.data_dir),
        Command::Nodes => commands::nodes(&cli.data_dir).await,
        Command::Run(args) => commands::run(args, cli.on.as_deref(), &cli.data_dir).await,
        Command::Ps { all } => commands::ps(all, cli.on.as_deref(), &cli.data_dir).await,
        Command::Stop { workload_id } => commands::stop(&workload_id, cli.on.as_deref(), &cli.data_dir).await,
        Command::Logs { workload_id } => commands::logs(&workload_id, cli.on.as_deref(), &cli.data_dir).await,
        Command::Put { path } => commands::put(&path, &cli.data_dir).await,
        Command::Get { id, out } => commands::get(&id, &out, &cli.data_dir).await,
        Command::Chunk { path } => commands::chunk(&path),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e:#}");
            ExitCode::FAILURE
        }
    }
}

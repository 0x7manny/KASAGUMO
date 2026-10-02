mod cli;
mod commands;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Node { action } => commands::node(action),
        Command::Nodes => commands::nodes(),
        Command::Run(args) => commands::run(args),
        Command::Ps { all } => commands::ps(all),
        Command::Stop { workload_id } => commands::stop(&workload_id),
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

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "kgo", version, about = "Kasagumo — un cloud décentralisé écrit en Rust")]
pub struct Cli {
    /// Dossier de données du nœud (contient le socket du daemon)
    #[arg(long, global = true, default_value = ".kasagumo")]
    pub data_dir: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Gérer le nœud local
    Node {
        #[command(subcommand)]
        action: NodeAction,
    },
    /// Lister les nœuds disponibles
    Nodes,
    /// Lancer un workload
    Run(RunArgs),
    /// Lister les workloads en cours
    Ps {
        /// Inclure les workloads arrêtés
        #[arg(short, long)]
        all: bool,
    },
    /// Arrêter un workload
    Stop {
        /// Identifiant du workload
        workload_id: String,
    },
    /// Découper un fichier en chunks et vérifier son intégrité
    Chunk {
        /// Fichier à découper
        path: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum NodeAction {
    /// Démarrer un nœud
    Start {
        /// Port d'écoute
        #[arg(long, default_value_t = 7070)]
        port: u16,
    },
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Nombre de cœurs CPU
    #[arg(long, default_value_t = 1)]
    pub cpu: u32,

    /// Mémoire allouée (ex : 512MB, 4GB)
    #[arg(long, default_value = "512MB", value_parser = parse_memory)]
    pub memory: u64,

    /// Image à exécuter (ex : nginx:latest)
    pub image: String,
}

pub fn parse_memory(input: &str) -> Result<u64, String> {
    let s = input.trim().to_ascii_uppercase();
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (number, unit) = s.split_at(split);

    let value: u64 = number
        .parse()
        .map_err(|_| format!("« {input} » : nombre invalide"))?;

    let multiplier: u64 = match unit.trim() {
        "" | "B" => 1,
        "K" | "KB" | "KIB" => 1 << 10,
        "M" | "MB" | "MIB" => 1 << 20,
        "G" | "GB" | "GIB" => 1 << 30,
        _ => return Err(format!("« {input} » : unité inconnue (B, KB, MB ou GB)")),
    };

    let bytes = value
        .checked_mul(multiplier)
        .ok_or_else(|| format!("« {input} » : valeur trop grande"))?;
    if bytes == 0 {
        return Err("la mémoire doit être supérieure à 0".to_string());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn configuration_clap_valide() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parse_memory_unites() {
        assert_eq!(parse_memory("1024"), Ok(1024));
        assert_eq!(parse_memory("512MB"), Ok(512 << 20));
        assert_eq!(parse_memory("4gb"), Ok(4 << 30));
    }

    #[test]
    fn parse_memory_refuse_les_valeurs_invalides() {
        assert!(parse_memory("4TB").is_err());
        assert!(parse_memory("0MB").is_err());
        assert!(parse_memory("abc").is_err());
        assert!(parse_memory("99999999999999GB").is_err());
    }

    #[test]
    fn commande_du_readme() {
        let cli = Cli::try_parse_from(["kgo", "run", "--cpu", "2", "--memory", "4GB", "nginx:latest"]).unwrap();
        match cli.command {
            Command::Run(a) => {
                assert_eq!((a.cpu, a.memory, a.image.as_str()), (2, 4 << 30, "nginx:latest"));
            }
            other => panic!("mauvaise commande : {other:?}"),
        }
    }

    #[test]
    fn run_sans_image_est_refuse() {
        assert!(Cli::try_parse_from(["kgo", "run", "--cpu", "2"]).is_err());
    }
}

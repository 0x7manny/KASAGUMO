use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "kgo", version, about = "Kasagumo — un cloud décentralisé écrit en Rust")]
pub struct Cli {
    /// Dossier de données du nœud (contient le socket du daemon)
    #[arg(long, global = true, default_value = ".kasagumo")]
    pub data_dir: PathBuf,

    /// Adresse d'un pair qui exécute `run`, `ps` ou `stop` à la place du nœud local
    #[arg(long, global = true)]
    pub on: Option<String>,

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
    /// Afficher les dernières lignes de sortie d'un workload
    Logs {
        /// Identifiant du workload
        workload_id: String,
    },
    /// Répartir un fichier sur le cluster et afficher son identifiant
    Put {
        /// Fichier à stocker
        path: PathBuf,
    },
    /// Récupérer un fichier du cluster à partir de son identifiant
    Get {
        /// Identifiant affiché par `kgo put`
        id: String,
        /// Fichier à écrire
        out: PathBuf,
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

        /// Secret partagé par les nœuds du cluster (sans lui, les pairs ne peuvent rien lancer)
        #[arg(long, env = "KGO_TOKEN", hide_env_values = true)]
        token: Option<String>,

        /// Délai entre deux battements de cœur vers les pairs (ms)
        #[arg(long, default_value_t = 5000)]
        heartbeat_ms: u64,
    },
    /// Ajouter un autre nœud à la liste des pairs
    Join {
        /// Adresse du pair (ex : 192.168.1.10:7070)
        addr: String,
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

    /// Port publié, `hôte:conteneur` (ex : 8080:80), répétable
    #[arg(short = 'p', long = "publish", value_parser = parse_port)]
    pub ports: Vec<String>,

    /// Variable d'environnement, `CLÉ=valeur`, répétable
    #[arg(short = 'e', long = "env", value_parser = parse_env)]
    pub env: Vec<String>,

    /// Image à exécuter (ex : nginx:latest)
    pub image: String,
}

fn parse_port(input: &str) -> Result<String, String> {
    let valid = input
        .split_once(':')
        .is_some_and(|(host, container)| host.parse::<u16>().is_ok_and(|p| p > 0) && container.parse::<u16>().is_ok_and(|p| p > 0));
    if valid { Ok(input.to_string()) } else { Err(format!("« {input} » : attendu hôte:conteneur (ex : 8080:80)")) }
}

fn parse_env(input: &str) -> Result<String, String> {
    match input.split_once('=') {
        Some((key, _)) if !key.is_empty() => Ok(input.to_string()),
        _ => Err(format!("« {input} » : attendu CLÉ=valeur")),
    }
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
    fn ports_et_variables() {
        let cli = Cli::try_parse_from(["kgo", "run", "-p", "8080:80", "-e", "A=1", "-e", "B=", "nginx"]).unwrap();
        match cli.command {
            Command::Run(a) => assert_eq!((a.ports, a.env), (vec!["8080:80".to_string()], vec!["A=1".to_string(), "B=".to_string()])),
            other => panic!("mauvaise commande : {other:?}"),
        }
        for bad in [["-p", "80"], ["-p", "a:80"], ["-p", "0:80"], ["-e", "=x"], ["-e", "NOPE"]] {
            assert!(Cli::try_parse_from(["kgo", "run", bad[0], bad[1], "nginx"]).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn run_sans_image_est_refuse() {
        assert!(Cli::try_parse_from(["kgo", "run", "--cpu", "2"]).is_err());
    }
}

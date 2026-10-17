use serde::{Deserialize, Serialize};

use crate::workload::{Workload, WorkloadSpec};

/// Deux nœuds parlent le même protocole s'ils partagent la version majeure — ou, avant la 1.0,
/// la même version mineure (convention semver : tout peut casser entre deux `0.x`).
pub fn versions_compatible(a: &str, b: &str) -> bool {
    fn key(v: &str) -> (u64, u64) {
        let mut parts = v.split(['.', '-', '+']).map(|p| p.parse::<u64>().unwrap_or(0));
        let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
        if major == 0 { (0, minor) } else { (major, 0) }
    }
    key(a) == key(b)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub version: String,
    pub cpus: u32,
    pub memory: u64,
    pub port: u16,
    /// Ressources réservées par les workloads actifs.
    pub used_cpus: u32,
    pub used_memory: u64,
}

impl NodeInfo {
    pub fn free_cpus(&self) -> u32 {
        self.cpus.saturating_sub(self.used_cpus)
    }

    pub fn free_memory(&self) -> u64 {
        self.memory.saturating_sub(self.used_memory)
    }

    pub fn fits(&self, spec: &WorkloadSpec) -> bool {
        spec.cpu <= self.free_cpus() && spec.memory <= self.free_memory()
    }
}

/// Un nœud tel que vu par `kgo nodes` : `info` est vide si le pair est injoignable.
#[derive(Debug, Serialize, Deserialize)]
pub struct NodeStatus {
    pub addr: String,
    pub info: Result<NodeInfo, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    Run(WorkloadSpec),
    /// Place le workload sur le nœud le plus libre du cluster (réservé au CLI local).
    Schedule(WorkloadSpec),
    Ps { all: bool },
    Stop { id: String },
    Logs { id: String },
    /// Range un bloc (hex) sur ce nœud.
    Store { data: String },
    /// Parmi ces blocs, lesquels manquent à ce nœud ?
    Missing { ids: Vec<String> },
    /// Demande un bloc à ce nœud.
    Fetch { id: String },
    /// Répartit un bloc (hex) sur `REPLICAS` nœuds du cluster (réservé au CLI local).
    Put { data: String },
    /// Récupère un bloc n'importe où dans le cluster (réservé au CLI local).
    Get { id: String },
    Info,
    AddPeer { addr: String },
    /// Oublie un pair (réservé au CLI local).
    RemovePeer { addr: String },
    Nodes,
    /// Battement de cœur d'un pair : annonce son port d'écoute et les workloads qu'il héberge,
    /// reçoit les pairs connus.
    Hello { port: u16, workloads: Vec<(String, WorkloadSpec)> },
    /// Workloads d'un pair tombé que l'expéditeur connaît : au chef de les relancer.
    Adopt { dead: String, workloads: Vec<(String, WorkloadSpec)> },
    /// Ces workloads ont été relancés ailleurs : à ne plus relancer.
    Forget { ids: Vec<String> },
    /// Fait exécuter la requête par un pair (le nœud local s'authentifie pour le CLI).
    Forward { addr: String, request: Box<Request> },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Started(Workload),
    Placed { addr: String, workload: Workload },
    Workloads(Vec<Workload>),
    Stopped,
    Logs(String),
    Stored { id: String, copies: usize },
    Blob(Option<String>),
    Ids(Vec<String>),
    Info(NodeInfo),
    PeerAdded,
    PeerRemoved { known: bool },
    Peers(Vec<String>),
    Nodes(Vec<NodeStatus>),
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::versions_compatible;

    #[test]
    fn compatibilite_des_versions() {
        assert!(versions_compatible("0.1.0", "0.1.3"), "même mineure avant 1.0");
        assert!(!versions_compatible("0.1.0", "0.2.0"), "mineure différente avant 1.0");
        assert!(versions_compatible("1.2.0", "1.9.4"), "même majeure");
        assert!(!versions_compatible("1.0.0", "2.0.0"), "majeure différente");
    }
}

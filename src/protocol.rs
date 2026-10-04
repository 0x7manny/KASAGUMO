use serde::{Deserialize, Serialize};

use crate::workload::{Workload, WorkloadSpec};

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
    Info,
    AddPeer { addr: String },
    Nodes,
    /// Battement de cœur d'un pair : annonce son port d'écoute, reçoit les pairs connus.
    Hello { port: u16 },
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
    Info(NodeInfo),
    PeerAdded,
    Peers(Vec<String>),
    Nodes(Vec<NodeStatus>),
    Error(String),
}

use serde::{Deserialize, Serialize};

use crate::workload::{Workload, WorkloadSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub version: String,
    pub cpus: u32,
    pub port: u16,
}

/// Un nœud tel que vu par `kgo nodes` : `info` est vide si le pair est injoignable.
#[derive(Debug, Serialize, Deserialize)]
pub struct NodeStatus {
    pub addr: String,
    pub info: Result<NodeInfo, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Run(WorkloadSpec),
    Ps { all: bool },
    Stop { id: String },
    Info,
    AddPeer { addr: String },
    Nodes,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Started(Workload),
    Workloads(Vec<Workload>),
    Stopped,
    Info(NodeInfo),
    PeerAdded,
    Nodes(Vec<NodeStatus>),
    Error(String),
}

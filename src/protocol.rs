use serde::{Deserialize, Serialize};

use crate::workload::{Workload, WorkloadSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub version: String,
    pub cpus: u32,
    pub port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Run(WorkloadSpec),
    Ps { all: bool },
    Stop { id: String },
    Info,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Started(Workload),
    Workloads(Vec<Workload>),
    Stopped,
    Info(NodeInfo),
    Error(String),
}

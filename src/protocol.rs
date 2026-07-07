use serde::{Deserialize, Serialize};

use crate::workload::{Workload, WorkloadSpec};

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Run(WorkloadSpec),
    Ps { all: bool },
    Stop { id: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Started(Workload),
    Workloads(Vec<Workload>),
    Stopped,
    Error(String),
}

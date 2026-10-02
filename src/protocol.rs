use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Ps { all: bool },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Workloads(Vec<WorkloadInfo>),
    Error(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkloadInfo {
    pub id: String,
    pub image: String,
    pub status: String,
}

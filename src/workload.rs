use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadSpec {
    pub image: String,
    pub cpu: u32,
    pub memory: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkloadState {
    Pending,
    Pulling,
    Running,
    Stopped,
    Failed,
}

impl WorkloadState {
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Stopped | Self::Failed)
    }
}

impl fmt::Display for WorkloadState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let name = match self {
            Self::Pending => "pending",
            Self::Pulling => "pulling",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        };
        f.write_str(name)
    }
}

/// Un workload que ce nœud a confié à un pair, pour le relancer ailleurs si le pair tombe.
#[derive(Debug, Serialize, Deserialize)]
pub struct Placement {
    pub addr: String,
    pub spec: WorkloadSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workload {
    pub id: String,
    pub spec: WorkloadSpec,
    pub state: WorkloadState,
}

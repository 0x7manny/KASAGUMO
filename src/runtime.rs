use anyhow::{Context, bail};
use tokio::process::Command;

use crate::workload::Workload;

/// Nom du conteneur Docker associé à un workload.
pub fn container_name(workload_id: &str) -> String {
    format!("kgo-{workload_id}")
}

/// Arguments de `docker run` pour un workload (limites CPU/mémoire incluses).
pub fn run_args(workload: &Workload) -> Vec<String> {
    vec![
        "run".into(),
        "--detach".into(),
        "--name".into(),
        container_name(&workload.id),
        "--cpus".into(),
        workload.spec.cpu.to_string(),
        "--memory".into(),
        workload.spec.memory.to_string(),
        workload.spec.image.clone(),
    ]
}

/// Exécute les workloads en pilotant la CLI `docker`.
pub struct DockerRuntime;

impl DockerRuntime {
    pub async fn pull(&self, image: &str) -> anyhow::Result<()> {
        docker(["pull", image]).await.map(drop)
    }

    pub async fn start(&self, workload: &Workload) -> anyhow::Result<()> {
        docker(run_args(workload)).await.map(drop)
    }

    pub async fn stop(&self, workload_id: &str) -> anyhow::Result<()> {
        let name = container_name(workload_id);
        docker(["rm", "--force", name.as_str()]).await.map(drop)
    }
}

async fn docker<I, S>(args: I) -> anyhow::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let args: Vec<S> = args.into_iter().collect();
    let output = Command::new("docker")
        .args(&args)
        .output()
        .await
        .context("impossible de lancer `docker` (est-il installé ?)")?;
    if !output.status.success() {
        bail!("docker {} : {}", args[0].as_ref().to_string_lossy(), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workload::{WorkloadSpec, WorkloadState};

    #[test]
    fn arguments_docker_avec_limites() {
        let workload = Workload {
            id: "ab12cd34".into(),
            spec: WorkloadSpec { image: "nginx:latest".into(), cpu: 2, memory: 4 << 30 },
            state: WorkloadState::Pending,
        };
        assert_eq!(
            run_args(&workload),
            ["run", "--detach", "--name", "kgo-ab12cd34", "--cpus", "2", "--memory", "4294967296", "nginx:latest"]
        );
    }
}

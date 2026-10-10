use anyhow::{Context, bail};
use tokio::process::Command;

use crate::workload::Workload;

/// Nom du conteneur Docker associé à un workload.
pub fn container_name(workload_id: &str) -> String {
    format!("kgo-{workload_id}")
}

/// Arguments de `docker run` pour un workload (limites CPU/mémoire incluses).
pub fn run_args(workload: &Workload) -> Vec<String> {
    let spec = &workload.spec;
    let mut args = vec![
        "run".to_string(),
        "--detach".into(),
        "--name".into(),
        container_name(&workload.id),
        "--cpus".into(),
        spec.cpu.to_string(),
        "--memory".into(),
        spec.memory.to_string(),
    ];
    for (flag, values) in [("--publish", &spec.ports), ("--env", &spec.env)] {
        for value in values {
            args.extend([flag.to_string(), value.clone()]);
        }
    }
    args.push(spec.image.clone());
    args
}

/// Exécute les workloads en pilotant la CLI `docker` (remplaçable via `KGO_DOCKER`).
pub struct DockerRuntime;

impl DockerRuntime {
    pub async fn pull(&self, image: &str) -> anyhow::Result<()> {
        docker(["pull", image]).await.map(drop)
    }

    pub async fn start(&self, workload: &Workload) -> anyhow::Result<()> {
        docker(run_args(workload)).await.map(drop)
    }

    /// Vrai si le conteneur du workload existe et tourne.
    pub async fn is_running(&self, workload_id: &str) -> bool {
        let name = container_name(workload_id);
        docker(["inspect", "--format", "{{.State.Running}}", name.as_str()])
            .await
            .is_ok_and(|out| out == "true")
    }

    /// Les dernières lignes de sortie du conteneur (stdout puis stderr).
    pub async fn logs(&self, workload_id: &str) -> anyhow::Result<String> {
        let name = container_name(workload_id);
        let output = run_docker(["logs", "--tail", "200", name.as_str()]).await?;
        Ok(String::from_utf8_lossy(&[output.stdout, output.stderr].concat()).into_owned())
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
    let output = run_docker(args).await?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn run_docker<I, S>(args: I) -> anyhow::Result<std::process::Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let args: Vec<S> = args.into_iter().collect();
    let program = std::env::var("KGO_DOCKER").unwrap_or_else(|_| "docker".to_string());
    let output = Command::new(program)
        .args(&args)
        .output()
        .await
        .context("impossible de lancer `docker` (est-il installé ?)")?;
    if !output.status.success() {
        bail!("docker {} : {}", args[0].as_ref().to_string_lossy(), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workload::{WorkloadSpec, WorkloadState};

    #[test]
    fn arguments_docker_avec_limites() {
        let workload = Workload {
            id: "ab12cd34".into(),
            spec: WorkloadSpec {
                image: "nginx:latest".into(),
                cpu: 2,
                memory: 4 << 30,
                ports: vec!["8080:80".into()],
                env: vec!["MODE=prod".into()],
            },
            state: WorkloadState::Pending,
        };
        assert_eq!(
            run_args(&workload),
            [
                "run", "--detach", "--name", "kgo-ab12cd34", "--cpus", "2", "--memory", "4294967296",
                "--publish", "8080:80", "--env", "MODE=prod", "nginx:latest"
            ]
        );
    }
}

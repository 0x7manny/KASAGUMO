use std::path::Path;

use anyhow::{Context, bail};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::workload::{Workload, WorkloadSpec, WorkloadState};

const WORKLOADS: TableDefinition<&str, &[u8]> = TableDefinition::new("workloads");

pub struct Store {
    db: Database,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = Database::create(path)
            .with_context(|| format!("impossible d'ouvrir la base {}", path.display()))?;
        let tx = db.begin_write()?;
        tx.open_table(WORKLOADS)?;
        tx.commit()?;
        Ok(Self { db })
    }

    pub fn create(&self, spec: WorkloadSpec) -> anyhow::Result<Workload> {
        let tx = self.db.begin_write()?;
        let workload = {
            let mut table = tx.open_table(WORKLOADS)?;
            let id = loop {
                let candidate = format!("{:08x}", rand::random::<u32>());
                if table.get(candidate.as_str())?.is_none() {
                    break candidate;
                }
            };
            let workload = Workload {
                id,
                spec,
                state: WorkloadState::Pending,
            };
            table.insert(workload.id.as_str(), serde_json::to_vec(&workload)?.as_slice())?;
            workload
        };
        tx.commit()?;
        Ok(workload)
    }

    pub fn list(&self) -> anyhow::Result<Vec<Workload>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(WORKLOADS)?;
        let mut workloads = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            workloads.push(serde_json::from_slice(value.value())?);
        }
        Ok(workloads)
    }

    pub fn set_state(&self, id: &str, state: WorkloadState) -> anyhow::Result<()> {
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(WORKLOADS)?;
            let current = match table.get(id)? {
                Some(value) => serde_json::from_slice::<Workload>(value.value())?,
                None => bail!("workload introuvable : {id}"),
            };
            let updated = Workload { state, ..current };
            table.insert(id, serde_json::to_vec(&updated)?.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }
}

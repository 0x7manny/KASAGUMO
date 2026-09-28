use std::path::Path;

use anyhow::{Context, bail};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::workload::{Placement, Workload, WorkloadSpec, WorkloadState};

const WORKLOADS: TableDefinition<&str, &[u8]> = TableDefinition::new("workloads");
const PLACEMENTS: TableDefinition<&str, &[u8]> = TableDefinition::new("placements");
const ORPHANS: TableDefinition<&str, &str> = TableDefinition::new("orphans");
const PEERS: TableDefinition<&str, ()> = TableDefinition::new("peers");
const META: TableDefinition<&str, &str> = TableDefinition::new("meta");

pub struct Store {
    db: Database,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = Database::create(path)
            .with_context(|| format!("impossible d'ouvrir la base {}", path.display()))?;
        let tx = db.begin_write()?;
        tx.open_table(WORKLOADS)?;
        tx.open_table(META)?;
        tx.open_table(PEERS)?;
        tx.open_table(PLACEMENTS)?;
        tx.open_table(ORPHANS)?;
        tx.commit()?;
        Ok(Self { db })
    }

    /// Identifiant stable du nœud, généré au premier démarrage.
    pub fn node_id(&self) -> anyhow::Result<String> {
        let tx = self.db.begin_write()?;
        let id = {
            let mut meta = tx.open_table(META)?;
            let existing = meta.get("node_id")?.map(|v| v.value().to_string());
            match existing {
                Some(id) => id,
                None => {
                    let id = format!("{:016x}", rand::random::<u64>());
                    meta.insert("node_id", id.as_str())?;
                    id
                }
            }
        };
        tx.commit()?;
        Ok(id)
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

    /// Passe le workload à `to` seulement s'il est encore à l'état `from`.
    /// Renvoie false si l'état a changé entre-temps (ex : arrêt pendant le pull).
    pub fn advance(&self, id: &str, from: WorkloadState, to: WorkloadState) -> anyhow::Result<bool> {
        let tx = self.db.begin_write()?;
        let advanced = {
            let mut table = tx.open_table(WORKLOADS)?;
            let current = match table.get(id)? {
                Some(value) => serde_json::from_slice::<Workload>(value.value())?,
                None => bail!("workload introuvable : {id}"),
            };
            let advanced = current.state == from;
            if advanced {
                let updated = Workload { state: to, ..current };
                table.insert(id, serde_json::to_vec(&updated)?.as_slice())?;
            }
            advanced
        };
        tx.commit()?;
        Ok(advanced)
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

    pub fn place(&self, id: &str, placement: &Placement) -> anyhow::Result<()> {
        let tx = self.db.begin_write()?;
        tx.open_table(PLACEMENTS)?.insert(id, serde_json::to_vec(placement)?.as_slice())?;
        tx.commit()?;
        Ok(())
    }

    pub fn unplace(&self, id: &str) -> anyhow::Result<()> {
        let tx = self.db.begin_write()?;
        tx.open_table(PLACEMENTS)?.remove(id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn placement(&self, id: &str) -> anyhow::Result<Option<Placement>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(PLACEMENTS)?;
        let value = table.get(id)?;
        Ok(value.map(|v| serde_json::from_slice(v.value())).transpose()?)
    }

    pub fn placements(&self) -> anyhow::Result<Vec<(String, Placement)>> {
        let tx = self.db.begin_read()?;
        let mut placements = Vec::new();
        for entry in tx.open_table(PLACEMENTS)?.iter()? {
            let (id, value) = entry?;
            placements.push((id.value().to_string(), serde_json::from_slice(value.value())?));
        }
        Ok(placements)
    }

    /// Retient qu'un workload tourne peut-être encore sur `addr`, déclaré tombé.
    pub fn orphan(&self, id: &str, addr: &str) -> anyhow::Result<()> {
        let tx = self.db.begin_write()?;
        tx.open_table(ORPHANS)?.insert(id, addr)?;
        tx.commit()?;
        Ok(())
    }

    pub fn forget_orphan(&self, id: &str) -> anyhow::Result<()> {
        let tx = self.db.begin_write()?;
        tx.open_table(ORPHANS)?.remove(id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn orphans(&self) -> anyhow::Result<Vec<(String, String)>> {
        let tx = self.db.begin_read()?;
        let mut orphans = Vec::new();
        for entry in tx.open_table(ORPHANS)?.iter()? {
            let (id, addr) = entry?;
            orphans.push((id.value().to_string(), addr.value().to_string()));
        }
        Ok(orphans)
    }

    pub fn add_peer(&self, addr: &str) -> anyhow::Result<()> {
        let tx = self.db.begin_write()?;
        tx.open_table(PEERS)?.insert(addr, ())?;
        tx.commit()?;
        Ok(())
    }

    pub fn peers(&self) -> anyhow::Result<Vec<String>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(PEERS)?;
        let mut peers = Vec::new();
        for entry in table.iter()? {
            let (addr, _) = entry?;
            peers.push(addr.value().to_string());
        }
        Ok(peers)
    }
}

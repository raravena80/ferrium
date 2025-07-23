use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::path::Path;
use std::sync::Arc;

use openraft::{
    storage::{Adaptor, LogState, RaftLogReader, RaftSnapshotBuilder, RaftStorage, Snapshot},
    Entry, EntryPayload, LogId, OptionalSend, SnapshotMeta, StorageError, StorageIOError,
    StoredMembership, Vote,
};
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, Direction, Options, DB};
use serde::{Deserialize, Serialize};

use tracing::info;

use crate::config::{KvRequest, KvResponse, KvSnapshot, Node, NodeId, TypeConfig};

const CF_LOG: &str = "log";
const CF_STATE: &str = "state";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KvStateMachine {
    pub data: BTreeMap<String, String>,
    pub last_applied_log_id: Option<LogId<NodeId>>,
    pub last_membership: StoredMembership<NodeId, Node>,
}

/// Unified storage implementation that implements the RaftStorage trait
#[derive(Debug)]
pub struct FerriiteStorage {
    db: Arc<DB>,
    state_machine: KvStateMachine,
}

// Helper functions for key conversion
fn id_to_bin(id: u64) -> Vec<u8> {
    use byteorder::{BigEndian, WriteBytesExt};
    let mut buf = Vec::with_capacity(8);
    buf.write_u64::<BigEndian>(id).unwrap();
    buf
}

fn bin_to_id(buf: &[u8]) -> u64 {
    use byteorder::{BigEndian, ReadBytesExt};
    (&buf[0..8]).read_u64::<BigEndian>().unwrap()
}

impl FerriiteStorage {
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, StorageError<NodeId>> {
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        let log_cf = ColumnFamilyDescriptor::new(CF_LOG, Options::default());
        let state_cf = ColumnFamilyDescriptor::new(CF_STATE, Options::default());

        let db = DB::open_cf_descriptors(&db_opts, db_path, vec![log_cf, state_cf])
            .map_err(|e| StorageIOError::<NodeId>::write(&e))?;
        let db = Arc::new(db);

        // Load existing state machine if it exists
        let mut storage = Self {
            db,
            state_machine: KvStateMachine::default(),
        };

        // Try to load existing state machine
        if let Some(snapshot) = storage.get_current_snapshot().await? {
            storage.load_snapshot_data(&snapshot.snapshot).await?;
        }

        Ok(storage)
    }

    fn logs(&self) -> &ColumnFamily {
        self.db.cf_handle(CF_LOG).unwrap()
    }

    fn state(&self) -> &ColumnFamily {
        self.db.cf_handle(CF_STATE).unwrap()
    }

    async fn load_snapshot_data(
        &mut self,
        snapshot_data: &Cursor<Vec<u8>>,
    ) -> Result<(), StorageError<NodeId>> {
        let kv_snapshot: KvSnapshot = serde_json::from_slice(snapshot_data.get_ref())
            .map_err(|e| StorageIOError::<NodeId>::read(&e))?;

        self.state_machine.data = kv_snapshot.data.into_iter().collect();
        Ok(())
    }

    #[allow(clippy::result_large_err)]
    fn get_last_purged_(&self) -> Result<Option<LogId<NodeId>>, StorageError<NodeId>> {
        Ok(self
            .db
            .get_cf(self.state(), b"last_purged_log_id")
            .map_err(|e| StorageIOError::<NodeId>::read(&e))?
            .and_then(|v| serde_json::from_slice(&v).ok()))
    }

    #[allow(clippy::result_large_err)]
    fn set_last_purged_(&self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        self.db
            .put_cf(
                self.state(),
                b"last_purged_log_id",
                serde_json::to_vec(&log_id).unwrap(),
            )
            .map_err(|e| StorageError::from(StorageIOError::<NodeId>::write(&e)))
    }
}

impl Clone for FerriiteStorage {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            state_machine: self.state_machine.clone(),
        }
    }
}

impl RaftLogReader<TypeConfig> for FerriiteStorage {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<TypeConfig>>, StorageError<NodeId>> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => id_to_bin(*x),
            std::ops::Bound::Excluded(x) => id_to_bin(*x + 1),
            std::ops::Bound::Unbounded => id_to_bin(0),
        };

        self.db
            .iterator_cf(
                self.logs(),
                rocksdb::IteratorMode::From(&start, Direction::Forward),
            )
            .map(|res| {
                let (id, val) = res.map_err(|e| StorageIOError::<NodeId>::read(&e))?;
                let entry: Entry<TypeConfig> =
                    serde_json::from_slice(&val).map_err(|e| StorageIOError::<NodeId>::read(&e))?;
                let id = bin_to_id(&id);

                assert_eq!(id, entry.log_id.index);
                Ok((id, entry))
            })
            .take_while(|res| res.as_ref().map_or(true, |(id, _)| range.contains(id)))
            .map(|res| res.map(|(_, entry)| entry))
            .collect()
    }
}

pub struct FerriiteSnapshotBuilder {
    storage: FerriiteStorage,
}

impl RaftSnapshotBuilder<TypeConfig> for FerriiteSnapshotBuilder {
    async fn build_snapshot(&mut self) -> Result<Snapshot<TypeConfig>, StorageError<NodeId>> {
        let last_applied_log = self.storage.state_machine.last_applied_log_id;
        let last_membership = self.storage.state_machine.last_membership.clone();

        let snapshot_data = KvSnapshot {
            data: self
                .storage
                .state_machine
                .data
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        };

        let kv_json = serde_json::to_vec(&snapshot_data)
            .map_err(|e| StorageIOError::<NodeId>::read_state_machine(&e))?;

        let snapshot_id = if let Some(last) = last_applied_log {
            format!("{}-{}", last.leader_id, last.index)
        } else {
            "empty".to_string()
        };

        let meta = SnapshotMeta {
            last_log_id: last_applied_log,
            last_membership,
            snapshot_id,
        };

        // Store the snapshot
        self.storage
            .db
            .put_cf(
                self.storage.state(),
                b"snapshot",
                serde_json::to_vec(&(meta.clone(), kv_json.clone())).unwrap(),
            )
            .map_err(|e| StorageIOError::<NodeId>::write(&e))?;

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(kv_json)),
        })
    }
}

impl RaftStorage<TypeConfig> for FerriiteStorage {
    type LogReader = Self;
    type SnapshotBuilder = FerriiteSnapshotBuilder;

    async fn save_vote(&mut self, vote: &Vote<NodeId>) -> Result<(), StorageError<NodeId>> {
        self.db
            .put_cf(self.state(), b"vote", serde_json::to_vec(vote).unwrap())
            .map_err(|e| StorageIOError::<NodeId>::write_vote(&e).into())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        Ok(self
            .db
            .get_cf(self.state(), b"vote")
            .map_err(|e| StorageIOError::<NodeId>::read_vote(&e))?
            .and_then(|v| serde_json::from_slice(&v).ok()))
    }

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<NodeId>> {
        let last = self
            .db
            .iterator_cf(self.logs(), rocksdb::IteratorMode::End)
            .next()
            .and_then(|res| {
                let (_, ent) = res.ok()?;
                Some(
                    serde_json::from_slice::<Entry<TypeConfig>>(&ent)
                        .ok()?
                        .log_id,
                )
            });

        let last_purged_log_id = self.get_last_purged_()?;

        let last_log_id = match last {
            None => last_purged_log_id,
            Some(x) => Some(x),
        };

        Ok(LogState {
            last_purged_log_id,
            last_log_id,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + OptionalSend,
    {
        for entry in entries {
            let id = id_to_bin(entry.log_id.index);
            self.db
                .put_cf(
                    self.logs(),
                    id,
                    serde_json::to_vec(&entry).map_err(|e| StorageIOError::<NodeId>::write(&e))?,
                )
                .map_err(|e| StorageIOError::<NodeId>::write(&e))?;
        }

        Ok(())
    }

    async fn delete_conflict_logs_since(
        &mut self,
        log_id: LogId<NodeId>,
    ) -> Result<(), StorageError<NodeId>> {
        tracing::debug!("delete_conflict_logs_since: [{:?}, +oo)", log_id);

        let from = id_to_bin(log_id.index);
        let to = id_to_bin(0xff_ff_ff_ff_ff_ff_ff_ff);
        self.db
            .delete_range_cf(self.logs(), &from, &to)
            .map_err(|e| StorageIOError::<NodeId>::write(&e).into())
    }

    async fn purge_logs_upto(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        tracing::debug!("purge_logs_upto: [0, {:?}]", log_id);

        self.set_last_purged_(log_id)?;
        let from = id_to_bin(0);
        let to = id_to_bin(log_id.index + 1);
        self.db
            .delete_range_cf(self.logs(), &from, &to)
            .map_err(|e| StorageIOError::<NodeId>::write(&e).into())
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId<NodeId>>, StoredMembership<NodeId, Node>), StorageError<NodeId>> {
        Ok((
            self.state_machine.last_applied_log_id,
            self.state_machine.last_membership.clone(),
        ))
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry<TypeConfig>],
    ) -> Result<Vec<KvResponse>, StorageError<NodeId>> {
        let mut responses = Vec::with_capacity(entries.len());

        for entry in entries {
            self.state_machine.last_applied_log_id = Some(entry.log_id);

            match &entry.payload {
                EntryPayload::Blank => {
                    responses.push(KvResponse::Set);
                }
                EntryPayload::Normal(req) => match req {
                    KvRequest::Set { key, value } => {
                        self.state_machine.data.insert(key.clone(), value.clone());
                        responses.push(KvResponse::Set);
                    }
                    KvRequest::Get { key } => {
                        let value = self.state_machine.data.get(key).cloned();
                        responses.push(KvResponse::Get { value });
                    }
                    KvRequest::Delete { key } => {
                        self.state_machine.data.remove(key);
                        responses.push(KvResponse::Delete);
                    }
                },
                EntryPayload::Membership(mem) => {
                    self.state_machine.last_membership =
                        StoredMembership::new(Some(entry.log_id), mem.clone());
                    responses.push(KvResponse::Set);
                }
            }
        }
        Ok(responses)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        FerriiteSnapshotBuilder {
            storage: self.clone(),
        }
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<NodeId>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeId, Node>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<NodeId>> {
        info!("Installing snapshot: {:?}", meta);

        let snapshot_data: KvSnapshot = serde_json::from_slice(snapshot.get_ref())
            .map_err(|e| StorageIOError::<NodeId>::read_snapshot(Some(meta.signature()), &e))?;

        // Update state machine
        self.state_machine = KvStateMachine {
            data: snapshot_data.data.into_iter().collect(),
            last_applied_log_id: meta.last_log_id,
            last_membership: meta.last_membership.clone(),
        };

        // Save the snapshot
        self.db
            .put_cf(
                self.state(),
                b"snapshot",
                serde_json::to_vec(&(meta.clone(), snapshot.into_inner())).unwrap(),
            )
            .map_err(|e| StorageIOError::<NodeId>::write(&e))?;

        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<TypeConfig>>, StorageError<NodeId>> {
        match self
            .db
            .get_cf(self.state(), b"snapshot")
            .map_err(|e| StorageIOError::<NodeId>::read(&e))?
        {
            Some(data) => {
                let (meta, snapshot_data): (SnapshotMeta<NodeId, Node>, Vec<u8>) =
                    serde_json::from_slice(&data)
                        .map_err(|e| StorageIOError::<NodeId>::read(&e))?;
                Ok(Some(Snapshot {
                    meta,
                    snapshot: Box::new(Cursor::new(snapshot_data)),
                }))
            }
            None => Ok(None),
        }
    }
}

/// Create new storage instances using the Adaptor pattern
pub async fn new_storage<P: AsRef<Path>>(
    db_path: P,
) -> Result<
    (
        Adaptor<TypeConfig, FerriiteStorage>,
        Adaptor<TypeConfig, FerriiteStorage>,
    ),
    StorageError<NodeId>,
> {
    let storage = FerriiteStorage::new(db_path).await?;
    Ok(Adaptor::new(storage))
}

// Re-export the Adaptor types for convenience
pub type LogStore = Adaptor<TypeConfig, FerriiteStorage>;
pub type StateMachineStore = Adaptor<TypeConfig, FerriiteStorage>;

// Storage tests are in test.rs
#[cfg(test)]
mod test;

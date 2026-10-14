#![cfg(not(target_family = "wasm"))]

use nanocode_durability::{
    OwnedState, OwnerId, OwnerToken, StateStore, StoreError, StoreFuture, StoreRecord, StoredState,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    path::Path,
};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Entry {
    Acquire {
        state_id: String,
        owner: OwnerToken,
    },
    Replace {
        state_id: String,
        owner: OwnerToken,
        revision: u64,
        payload: String,
        records: Vec<StoreRecord>,
    },
}

pub struct JsonlStore {
    file: File,
    offset: u64,
    owners: HashMap<String, OwnerToken>,
    states: HashMap<String, StoredState>,
    records: HashMap<(String, String), String>,
    created: Vec<String>,
}

impl JsonlStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(backend)?;
        let mut store = Self {
            file,
            offset: 0,
            owners: HashMap::new(),
            states: HashMap::new(),
            records: HashMap::new(),
            created: Vec::new(),
        };
        store.locked(|_| Ok(()))?;
        Ok(store)
    }

    pub fn open_home(home: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(home).map_err(backend)?;
        Self::open(home.join("sessions.jsonl"))
    }

    fn locked<T>(
        &mut self,
        action: impl FnOnce(&mut Self) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        self.file.lock().map_err(backend)?;
        let result = self.refresh().and_then(|()| action(self));
        let unlocked = self.file.unlock().map_err(backend);
        match result {
            Ok(value) => unlocked.map(|()| value),
            Err(error) => Err(error),
        }
    }

    fn refresh(&mut self) -> Result<(), StoreError> {
        if self.file.metadata().map_err(backend)?.len() < self.offset {
            return Err(StoreError::Backend(
                "JSONL durability log was truncated before its committed offset".into(),
            ));
        }
        self.file
            .seek(SeekFrom::Start(self.offset))
            .map_err(backend)?;
        let mut reader = BufReader::new(self.file.try_clone().map_err(backend)?);
        let mut line = Vec::new();
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line).map_err(backend)? == 0 {
                break;
            }
            if line.last() != Some(&b'\n') {
                self.file.set_len(self.offset).map_err(backend)?;
                self.file.sync_all().map_err(backend)?;
                break;
            }
            let entry: Entry = serde_json::from_slice(&line).map_err(backend)?;
            self.apply(entry)?;
            self.offset += line.len() as u64;
        }
        Ok(())
    }

    fn apply(&mut self, entry: Entry) -> Result<(), StoreError> {
        match entry {
            Entry::Acquire { state_id, owner } => {
                let expected = self
                    .owners
                    .get(&state_id)
                    .map_or(Some(1), |previous| previous.fence().checked_add(1));
                if expected != Some(owner.fence()) {
                    return Err(StoreError::Backend(
                        "invalid JSONL owner fence sequence".into(),
                    ));
                }
                self.owners.insert(state_id, owner);
            }
            Entry::Replace {
                state_id,
                owner,
                revision,
                payload,
                records,
            } => {
                let expected = self
                    .states
                    .get(&state_id)
                    .map_or(Some(1), |previous| previous.revision.checked_add(1));
                if self.owners.get(&state_id) != Some(&owner) || expected != Some(revision) {
                    return Err(StoreError::Backend(
                        "invalid JSONL state revision or owner".into(),
                    ));
                }
                self.validate_records(&state_id, &records)?;
                if !self.states.contains_key(&state_id) {
                    self.created.push(state_id.clone());
                }
                for record in records {
                    self.records
                        .insert((state_id.clone(), record.key), record.value);
                }
                self.states.insert(
                    state_id,
                    StoredState {
                        revision,
                        payload: Some(payload),
                    },
                );
            }
        }
        Ok(())
    }

    fn validate_records(&self, state_id: &str, records: &[StoreRecord]) -> Result<(), StoreError> {
        let mut staged = HashMap::new();
        for record in records {
            let existing = self
                .records
                .get(&(state_id.to_owned(), record.key.clone()))
                .or_else(|| staged.get(&record.key).copied());
            if existing.is_some_and(|value| value != &record.value) {
                return Err(StoreError::NotCommitted(format!(
                    "durability record `{}` has conflicting content",
                    record.key
                )));
            }
            staged.insert(record.key.clone(), &record.value);
        }
        Ok(())
    }

    fn append(&mut self, entry: Entry) -> Result<(), StoreError> {
        let mut bytes = serde_json::to_vec(&entry).map_err(backend)?;
        bytes.push(b'\n');
        self.file
            .seek(SeekFrom::Start(self.offset))
            .map_err(backend)?;
        self.file.write_all(&bytes).map_err(backend)?;
        self.file.sync_all().map_err(backend)?;
        self.apply(entry)?;
        self.offset += bytes.len() as u64;
        Ok(())
    }
}

impl StateStore for JsonlStore {
    fn read_record<'a>(
        &'a mut self,
        state_id: &'a str,
        key: &'a str,
    ) -> StoreFuture<'a, Result<Option<String>, StoreError>> {
        let result = self.locked(|store| {
            Ok(store
                .records
                .get(&(state_id.to_owned(), key.to_owned()))
                .cloned())
        });
        Box::pin(async move { result })
    }
    fn read_records<'a>(
        &'a mut self,
        state_id: &'a str,
        keys: &'a [String],
    ) -> StoreFuture<'a, Result<Vec<Option<String>>, StoreError>> {
        let result = self.locked(|store| {
            Ok(keys
                .iter()
                .map(|key| {
                    store
                        .records
                        .get(&(state_id.to_owned(), key.clone()))
                        .cloned()
                })
                .collect())
        });
        Box::pin(async move { result })
    }
    fn peek<'a>(
        &'a mut self,
        state_id: &'a str,
    ) -> StoreFuture<'a, Result<StoredState, StoreError>> {
        let result =
            self.locked(|store| Ok(store.states.get(state_id).cloned().unwrap_or_default()));
        Box::pin(async move { result })
    }
    fn list_states<'a>(
        &'a mut self,
        limit: usize,
    ) -> StoreFuture<'a, Result<Vec<String>, StoreError>> {
        let result =
            self.locked(|store| Ok(store.created.iter().rev().take(limit).cloned().collect()));
        Box::pin(async move { result })
    }
    fn acquire<'a>(
        &'a mut self,
        state_id: &'a str,
        owner_id: OwnerId,
    ) -> StoreFuture<'a, Result<OwnedState, StoreError>> {
        let result = self.locked(|store| {
            let fence = store
                .owners
                .get(state_id)
                .map_or(Some(1), |owner| owner.fence().checked_add(1))
                .ok_or_else(|| StoreError::NotCommitted("JSONL owner fence overflow".into()))?;
            let owner = OwnerToken::new(owner_id, fence);
            store.append(Entry::Acquire {
                state_id: state_id.to_owned(),
                owner: owner.clone(),
            })?;
            Ok(OwnedState {
                owner,
                state: store.states.get(state_id).cloned().unwrap_or_default(),
            })
        });
        Box::pin(async move { result })
    }
    fn replace<'a>(
        &'a mut self,
        state_id: &'a str,
        owner: &'a OwnerToken,
        expected_revision: u64,
        payload: &'a str,
        records: &'a [StoreRecord],
    ) -> StoreFuture<'a, Result<u64, StoreError>> {
        let result = self.locked(|store| {
            if store.owners.get(state_id) != Some(owner) {
                return Err(StoreError::Fenced);
            }
            let actual = store.states.get(state_id).map_or(0, |state| state.revision);
            if actual != expected_revision {
                return Err(StoreError::Conflict {
                    expected: expected_revision,
                    actual,
                });
            }
            let revision = actual
                .checked_add(1)
                .ok_or_else(|| StoreError::NotCommitted("JSONL revision overflow".into()))?;
            store.validate_records(state_id, records)?;
            store.append(Entry::Replace {
                state_id: state_id.to_owned(),
                owner: owner.clone(),
                revision,
                payload: payload.to_owned(),
                records: records.to_vec(),
            })?;
            Ok(revision)
        });
        Box::pin(async move { result })
    }
}

fn backend(error: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(error.to_string())
}

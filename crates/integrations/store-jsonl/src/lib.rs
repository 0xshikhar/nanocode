#![cfg(not(target_family = "wasm"))]

use nanocode_durability::{
    OwnedState, OwnerId, OwnerToken, StateStore, StoreError, StoreFuture, StoreRecord, StoredState,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
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
    index: Option<Connection>,
    source: String,
    cached: Option<(i64, i64, Entry)>,
    offset: u64,
}

impl JsonlStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path.as_ref())
            .map_err(backend)?;
        file.lock().map_err(backend)?;
        let identity = identity(&file)?;
        let canonical = std::fs::canonicalize(path.as_ref()).map_err(backend)?;
        let mut index_path = canonical.into_os_string();
        index_path.push(".index.sqlite");
        let index = Connection::open(Path::new(&index_path)).map_err(backend)?;
        let mode: String = index
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .map_err(backend)?;
        if mode != "wal" {
            return Err(backend("JSONL index requires SQLite WAL mode"));
        }
        index.execute_batch("PRAGMA synchronous=NORMAL;
            CREATE TABLE IF NOT EXISTS meta (id INTEGER PRIMARY KEY CHECK(id=1), source TEXT NOT NULL, offset TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS owners (id TEXT PRIMARY KEY, token TEXT NOT NULL) WITHOUT ROWID;
            CREATE TABLE IF NOT EXISTS states (id TEXT PRIMARY KEY, revision TEXT NOT NULL, position INTEGER NOT NULL, length INTEGER NOT NULL, created INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS states_created ON states(created DESC);
            CREATE TABLE IF NOT EXISTS records (id TEXT NOT NULL, key TEXT NOT NULL, position INTEGER NOT NULL, length INTEGER NOT NULL, PRIMARY KEY(id,key)) WITHOUT ROWID;").map_err(backend)?;
        let previous: Option<String> = index
            .query_row("SELECT source FROM meta WHERE id=1", [], |row| row.get(0))
            .optional()
            .map_err(backend)?;
        if previous.as_deref() != Some(&identity) {
            let transaction = index.unchecked_transaction().map_err(backend)?;
            transaction.execute_batch("DELETE FROM owners; DELETE FROM states; DELETE FROM records; DELETE FROM meta;").map_err(backend)?;
            transaction
                .execute("INSERT INTO meta VALUES(1,?1,'0')", [&identity])
                .map_err(backend)?;
            transaction.commit().map_err(backend)?;
        }
        file.unlock().map_err(backend)?;
        let mut store = Self {
            file,
            index: Some(index),
            source: identity,
            cached: None,
            offset: 0,
        };
        store.locked(|_| Ok(()))?;
        Ok(store)
    }

    pub fn open_home(home: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(home).map_err(backend)?;
        Self::open(home.join("sessions.jsonl"))
    }

    fn index(&self) -> &Connection {
        self.index
            .as_ref()
            .expect("an open JSONL store has an index")
    }

    fn locked<T>(
        &mut self,
        action: impl FnOnce(&mut Self) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        self.file.lock().map_err(backend)?;
        let result = (|| {
            self.index()
                .execute_batch("BEGIN IMMEDIATE")
                .map_err(backend)?;
            let result = self.refresh().and_then(|()| action(self));
            match result {
                Ok(value) => {
                    self.index().execute_batch("COMMIT").map_err(backend)?;
                    Ok(value)
                }
                Err(error) => Err(error),
            }
        })();
        if result.is_err() {
            let _ = self.index().execute_batch("ROLLBACK");
        }
        let unlocked = self.file.unlock().map_err(backend);
        match result {
            Ok(value) => unlocked.map(|()| value),
            Err(error) => Err(error),
        }
    }

    fn refresh(&mut self) -> Result<(), StoreError> {
        let (source, offset): (String, String) = self
            .index()
            .query_row("SELECT source,offset FROM meta WHERE id=1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(backend)?;
        if source != self.source {
            return Err(backend(
                "JSONL log identity changed while its store was open",
            ));
        }
        self.offset = offset.parse().map_err(backend)?;
        let length = self.file.metadata().map_err(backend)?.len();
        if length < self.offset {
            return Err(backend(
                "JSONL durability log was truncated before its committed offset",
            ));
        }
        if length == self.offset {
            return Ok(());
        }
        let mut reader = BufReader::new(self.file.try_clone().map_err(backend)?);
        let mut line = Vec::new();
        while self.offset < length {
            reader.seek(SeekFrom::Start(self.offset)).map_err(backend)?;
            line.clear();
            reader.read_until(b'\n', &mut line).map_err(backend)?;
            if line.last() != Some(&b'\n') {
                self.cached = None;
                self.file.set_len(self.offset).map_err(backend)?;
                self.file.sync_all().map_err(backend)?;
                break;
            }
            let entry: Entry = serde_json::from_slice(&line).map_err(backend)?;
            self.apply(&entry, self.offset, line.len())?;
            self.offset = self
                .offset
                .checked_add(line.len() as u64)
                .ok_or_else(|| backend("JSONL offset overflow"))?;
        }
        self.save_offset()
    }

    fn save_offset(&self) -> Result<(), StoreError> {
        self.index()
            .execute(
                "UPDATE meta SET offset=?1 WHERE id=1",
                [self.offset.to_string()],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn owner(&self, id: &str) -> Result<Option<OwnerToken>, StoreError> {
        let token: Option<String> = self
            .index()
            .query_row("SELECT token FROM owners WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(backend)?;
        token
            .map(|value| serde_json::from_str(&value).map_err(backend))
            .transpose()
    }

    fn revision(&self, id: &str) -> Result<u64, StoreError> {
        let revision: Option<String> = self
            .index()
            .query_row("SELECT revision FROM states WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(backend)?;
        revision
            .map(|value| value.parse().map_err(backend))
            .transpose()
            .map(|value| value.unwrap_or(0))
    }

    fn entry(&mut self, position: i64, length: i64) -> Result<&Entry, StoreError> {
        if self
            .cached
            .as_ref()
            .is_some_and(|(at, len, _)| *at == position && *len == length)
        {
            return Ok(&self.cached.as_ref().expect("a cached entry exists").2);
        }
        let key = (position, length);
        let position = u64::try_from(position).map_err(backend)?;
        let length = usize::try_from(length).map_err(backend)?;
        self.file.seek(SeekFrom::Start(position)).map_err(backend)?;
        let mut bytes = vec![0; length];
        self.file.read_exact(&mut bytes).map_err(backend)?;
        let entry = serde_json::from_slice(&bytes).map_err(backend)?;
        self.cached = Some((key.0, key.1, entry));
        Ok(&self.cached.as_ref().expect("the entry was loaded").2)
    }

    fn state(&mut self, id: &str) -> Result<StoredState, StoreError> {
        let location = self
            .index()
            .query_row(
                "SELECT position,length FROM states WHERE id=?1",
                [id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(backend)?;
        match location {
            Some((position, length)) => match self.entry(position, length)? {
                Entry::Replace {
                    state_id,
                    revision,
                    payload,
                    ..
                } if state_id == id => Ok(StoredState {
                    revision: *revision,
                    payload: Some(payload.clone()),
                }),
                _ => Err(backend("JSONL state index does not reference its state")),
            },
            None => Ok(StoredState::default()),
        }
    }

    fn record(&mut self, id: &str, key: &str) -> Result<Option<String>, StoreError> {
        let location = self
            .index()
            .query_row(
                "SELECT position,length FROM records WHERE id=?1 AND key=?2",
                params![id, key],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(backend)?;
        match location {
            Some((position, length)) => match self.entry(position, length)? {
                Entry::Replace {
                    state_id, records, ..
                } if state_id == id => records
                    .iter()
                    .find(|record| record.key == key)
                    .map(|record| Some(record.value.clone()))
                    .ok_or_else(|| backend("JSONL record index does not reference its record")),
                _ => Err(backend("JSONL record index does not reference its state")),
            },
            None => Ok(None),
        }
    }

    fn validate_records(&mut self, id: &str, records: &[StoreRecord]) -> Result<(), StoreError> {
        let mut staged = HashMap::new();
        for record in records {
            let existing = self.record(id, &record.key)?;
            let retained = existing
                .as_ref()
                .or_else(|| staged.get(&record.key).copied());
            if retained.is_some_and(|value| value != &record.value) {
                return Err(StoreError::NotCommitted(format!(
                    "durability record `{}` has conflicting content",
                    record.key
                )));
            }
            staged.insert(record.key.clone(), &record.value);
        }
        Ok(())
    }

    fn apply(&mut self, entry: &Entry, position: u64, length: usize) -> Result<(), StoreError> {
        let position = i64::try_from(position).map_err(backend)?;
        let length = i64::try_from(length).map_err(backend)?;
        match entry {
            Entry::Acquire { state_id, owner } => {
                let expected = self
                    .owner(state_id)?
                    .map_or(Some(1), |previous| previous.fence().checked_add(1));
                if expected != Some(owner.fence()) {
                    return Err(backend("invalid JSONL owner fence sequence"));
                }
                self.index().execute("INSERT INTO owners VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET token=excluded.token", params![state_id,serde_json::to_string(owner).map_err(backend)?]).map_err(backend)?;
            }
            Entry::Replace {
                state_id,
                owner,
                revision,
                records,
                ..
            } => {
                if self.owner(state_id)?.as_ref() != Some(owner)
                    || self.revision(state_id)?.checked_add(1) != Some(*revision)
                {
                    return Err(backend("invalid JSONL state revision or owner"));
                }
                self.validate_records(state_id, records)?;
                self.index().execute("INSERT INTO states VALUES(?1,?2,?3,?4,?3) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,position=excluded.position,length=excluded.length", params![state_id,revision.to_string(),position,length]).map_err(backend)?;
                for record in records {
                    self.index().execute("INSERT INTO records VALUES(?1,?2,?3,?4) ON CONFLICT(id,key) DO NOTHING", params![state_id,record.key,position,length]).map_err(backend)?;
                }
            }
        }
        Ok(())
    }

    fn append(&mut self, entry: Entry) -> Result<(), StoreError> {
        let mut bytes = serde_json::to_vec(&entry).map_err(backend)?;
        bytes.push(b'\n');
        self.file
            .seek(SeekFrom::Start(self.offset))
            .map_err(backend)?;
        self.cached = None;
        self.file.write_all(&bytes).map_err(backend)?;
        self.file.sync_all().map_err(backend)?;
        self.apply(&entry, self.offset, bytes.len())?;
        self.offset = self
            .offset
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| backend("JSONL offset overflow"))?;
        self.save_offset()
    }
}

impl Drop for JsonlStore {
    fn drop(&mut self) {
        if self.file.lock().is_ok() {
            drop(self.index.take());
            let _ = self.file.unlock();
        }
    }
}

impl StateStore for JsonlStore {
    fn read_record<'a>(
        &'a mut self,
        id: &'a str,
        key: &'a str,
    ) -> StoreFuture<'a, Result<Option<String>, StoreError>> {
        let result = self.locked(|store| store.record(id, key));
        Box::pin(async move { result })
    }
    fn read_records<'a>(
        &'a mut self,
        id: &'a str,
        keys: &'a [String],
    ) -> StoreFuture<'a, Result<Vec<Option<String>>, StoreError>> {
        let result = self.locked(|store| {
            let mut grouped = HashMap::<(i64, i64), Vec<(usize, &String)>>::new();
            let mut values = vec![None; keys.len()];
            for (index, key) in keys.iter().enumerate() {
                let location = store
                    .index()
                    .query_row(
                        "SELECT position,length FROM records WHERE id=?1 AND key=?2",
                        params![id, key],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                    )
                    .optional()
                    .map_err(backend)?;
                if let Some(location) = location {
                    grouped.entry(location).or_default().push((index, key));
                }
            }
            for ((position, length), requested) in grouped {
                let Entry::Replace {
                    state_id, records, ..
                } = store.entry(position, length)?
                else {
                    return Err(backend(
                        "JSONL record index does not reference a replacement",
                    ));
                };
                if state_id != id {
                    return Err(backend("JSONL record index does not reference its state"));
                }
                let records: HashMap<_, _> = records
                    .iter()
                    .map(|record| (&record.key, &record.value))
                    .collect();
                for (index, key) in requested {
                    values[index] = Some(
                        (*records.get(key).ok_or_else(|| {
                            backend("JSONL record index does not reference its record")
                        })?)
                        .clone(),
                    );
                }
            }
            Ok(values)
        });
        Box::pin(async move { result })
    }
    fn peek<'a>(&'a mut self, id: &'a str) -> StoreFuture<'a, Result<StoredState, StoreError>> {
        let result = self.locked(|store| store.state(id));
        Box::pin(async move { result })
    }
    fn list_states<'a>(
        &'a mut self,
        limit: usize,
    ) -> StoreFuture<'a, Result<Vec<String>, StoreError>> {
        let result = self.locked(|store| {
            let mut query = store
                .index()
                .prepare("SELECT id FROM states ORDER BY created DESC")
                .map_err(backend)?;
            let rows = query
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(backend)?;
            rows.take(limit).map(|row| row.map_err(backend)).collect()
        });
        Box::pin(async move { result })
    }
    fn acquire<'a>(
        &'a mut self,
        id: &'a str,
        owner_id: OwnerId,
    ) -> StoreFuture<'a, Result<OwnedState, StoreError>> {
        let result = self.locked(|store| {
            let fence = store
                .owner(id)?
                .map_or(Some(1), |owner| owner.fence().checked_add(1))
                .ok_or_else(|| StoreError::NotCommitted("JSONL owner fence overflow".into()))?;
            let owner = OwnerToken::new(owner_id, fence);
            store.append(Entry::Acquire {
                state_id: id.into(),
                owner: owner.clone(),
            })?;
            Ok(OwnedState {
                owner,
                state: store.state(id)?,
            })
        });
        Box::pin(async move { result })
    }
    fn replace<'a>(
        &'a mut self,
        id: &'a str,
        owner: &'a OwnerToken,
        expected: u64,
        payload: &'a str,
        records: &'a [StoreRecord],
    ) -> StoreFuture<'a, Result<u64, StoreError>> {
        let result = self.locked(|store| {
            if store.owner(id)?.as_ref() != Some(owner) {
                return Err(StoreError::Fenced);
            }
            let actual = store.revision(id)?;
            if actual != expected {
                return Err(StoreError::Conflict { expected, actual });
            }
            let revision = actual
                .checked_add(1)
                .ok_or_else(|| StoreError::NotCommitted("JSONL revision overflow".into()))?;
            store.validate_records(id, records)?;
            store.append(Entry::Replace {
                state_id: id.into(),
                owner: owner.clone(),
                revision,
                payload: payload.into(),
                records: records.into(),
            })?;
            Ok(revision)
        });
        Box::pin(async move { result })
    }
}

#[cfg(unix)]
fn identity(file: &File) -> Result<String, StoreError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(backend)?;
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn identity(file: &File) -> Result<String, StoreError> {
    Ok(format!(
        "{:?}",
        file.metadata()
            .map_err(backend)?
            .created()
            .map_err(backend)?
    ))
}

fn backend(error: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(error.to_string())
}

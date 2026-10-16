#![cfg(not(target_family = "wasm"))]

use nanocode_durability_core::{
    OwnedState, OwnerId, OwnerToken, StateStore, StoreError, StoreFuture, StoreRecord, StoredState,
};
use serde::{Deserialize, Deserializer, Serialize, de};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs::{File, Metadata, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Instant,
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

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Acquire,
    Replace,
}

#[derive(Deserialize)]
struct Indexed<'a> {
    #[serde(rename = "type")]
    kind: Kind,
    #[serde(borrow)]
    state_id: Cow<'a, str>,
    owner: OwnerToken,
    revision: Option<u64>,
    payload: Option<Text>,
    #[serde(borrow)]
    records: Option<Vec<IndexedRecord<'a>>>,
}

#[derive(Deserialize)]
struct IndexedRecord<'a> {
    #[serde(borrow)]
    key: Cow<'a, str>,
    #[serde(rename = "value")]
    _value: Text,
}

struct Text;

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(TextVisitor)
    }
}

struct TextVisitor;

impl de::Visitor<'_> for TextVisitor {
    type Value = Text;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string")
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<Text, E> {
        Ok(Text)
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Location {
    position: u64,
    length: usize,
}

struct State {
    revision: u64,
    location: Location,
    records: HashMap<String, Location>,
}

pub struct JsonlStore {
    file: File,
    reads: File,
    path: PathBuf,
    source: String,
    offset: u64,
    owners: HashMap<String, OwnerToken>,
    states: HashMap<String, State>,
    created: Vec<String>,
    cached: Option<(Location, Entry)>,
}

impl JsonlStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let _span = tracing::info_span!("durability.jsonl.open").entered();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path.as_ref())
            .map_err(backend)?;
        let path = std::fs::canonicalize(path.as_ref()).map_err(backend)?;
        let reads = File::open(&path).map_err(backend)?;
        let source = identity(&file.metadata().map_err(backend)?)?;
        if identity(&reads.metadata().map_err(backend)?)? != source {
            return Err(identity_changed());
        }
        let mut store = Self {
            file,
            reads,
            path,
            source,
            offset: 0,
            owners: HashMap::new(),
            states: HashMap::new(),
            created: Vec::new(),
            cached: None,
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
        let _span = tracing::info_span!("durability.jsonl.transaction").entered();
        self.file.lock().map_err(backend)?;
        let result = self.refresh().and_then(|()| action(self));
        let unlocked = self.file.unlock().map_err(backend);
        match result {
            Ok(value) => unlocked.map(|()| value),
            Err(error) => Err(error),
        }
    }

    fn refresh(&mut self) -> Result<(), StoreError> {
        if identity(&std::fs::metadata(&self.path).map_err(backend)?)? != self.source {
            return Err(identity_changed());
        }
        let length = self.file.metadata().map_err(backend)?.len();
        if length < self.offset {
            return Err(backend(
                "JSONL durability log was truncated before its committed offset",
            ));
        }
        if length == self.offset {
            return Ok(());
        }
        let _span = tracing::info_span!(
            "durability.jsonl.recover",
            log.unindexed_bytes = length - self.offset
        )
        .entered();
        let mut reader = BufReader::new(self.file.try_clone().map_err(backend)?);
        reader.seek(SeekFrom::Start(self.offset)).map_err(backend)?;
        let mut line = Vec::new();
        while self.offset < length {
            line.clear();
            reader.read_until(b'\n', &mut line).map_err(backend)?;
            if line.last() != Some(&b'\n') {
                self.file.set_len(self.offset).map_err(backend)?;
                self.file.sync_all().map_err(backend)?;
                break;
            }
            self.replay(&line)?;
            self.offset = self
                .offset
                .checked_add(line.len() as u64)
                .ok_or_else(|| backend("JSONL offset overflow"))?;
        }
        Ok(())
    }

    fn replay(&mut self, line: &[u8]) -> Result<(), StoreError> {
        let indexed: Indexed = serde_json::from_slice(line).map_err(backend)?;
        let location = Location {
            position: self.offset,
            length: line.len(),
        };
        match indexed.kind {
            Kind::Acquire => self.acquired(&indexed.state_id, indexed.owner),
            Kind::Replace => {
                let (Some(revision), Some(Text), Some(records)) =
                    (indexed.revision, indexed.payload, indexed.records)
                else {
                    return Err(backend("invalid JSONL replacement entry"));
                };
                let id = &*indexed.state_id;
                self.check_replacement(id, &indexed.owner, revision)?;
                let known = self.states.get(id).map(|state| &state.records);
                let mut keys = HashSet::with_capacity(records.len());
                let repeated = records.iter().any(|record| {
                    !keys.insert(&*record.key)
                        || known.is_some_and(|known| known.contains_key(&*record.key))
                });
                if repeated {
                    let Entry::Replace { records, .. } =
                        serde_json::from_slice(line).map_err(backend)?
                    else {
                        return Err(backend("invalid JSONL replacement entry"));
                    };
                    self.validate_records(id, &records)?;
                }
                self.replaced(
                    id,
                    revision,
                    location,
                    records.iter().map(|record| &*record.key),
                );
                Ok(())
            }
        }
    }

    fn next_fence(&self, id: &str) -> Option<u64> {
        self.owners
            .get(id)
            .map_or(Some(1), |owner| owner.fence().checked_add(1))
    }

    fn revision(&self, id: &str) -> u64 {
        self.states.get(id).map_or(0, |state| state.revision)
    }

    fn acquired(&mut self, id: &str, owner: OwnerToken) -> Result<(), StoreError> {
        if self.next_fence(id) != Some(owner.fence()) {
            return Err(backend("invalid JSONL owner fence sequence"));
        }
        match self.owners.get_mut(id) {
            Some(current) => *current = owner,
            None => {
                self.owners.insert(id.into(), owner);
            }
        }
        Ok(())
    }

    fn check_replacement(
        &self,
        id: &str,
        owner: &OwnerToken,
        revision: u64,
    ) -> Result<(), StoreError> {
        if self.owners.get(id) != Some(owner) || self.revision(id).checked_add(1) != Some(revision)
        {
            return Err(backend("invalid JSONL state revision or owner"));
        }
        Ok(())
    }

    fn replaced<'k>(
        &mut self,
        id: &str,
        revision: u64,
        location: Location,
        keys: impl Iterator<Item = &'k str>,
    ) {
        if !self.states.contains_key(id) {
            self.created.push(id.into());
            self.states.insert(
                id.into(),
                State {
                    revision,
                    location,
                    records: HashMap::new(),
                },
            );
        }
        let state = self
            .states
            .get_mut(id)
            .expect("a replaced state is indexed");
        state.revision = revision;
        state.location = location;
        for key in keys {
            if !state.records.contains_key(key) {
                state.records.insert(key.into(), location);
            }
        }
    }

    fn entry(&mut self, location: Location) -> Result<&Entry, StoreError> {
        if self
            .cached
            .as_ref()
            .is_none_or(|(cached, _)| *cached != location)
        {
            self.reads
                .seek(SeekFrom::Start(location.position))
                .map_err(backend)?;
            let mut bytes = vec![0; location.length];
            self.reads.read_exact(&mut bytes).map_err(backend)?;
            let entry = serde_json::from_slice(&bytes).map_err(backend)?;
            self.cached = Some((location, entry));
        }
        Ok(&self.cached.as_ref().expect("the entry was loaded").1)
    }

    fn state(&mut self, id: &str) -> Result<StoredState, StoreError> {
        let Some(location) = self.states.get(id).map(|state| state.location) else {
            return Ok(StoredState::default());
        };
        match self.entry(location)? {
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
        }
    }

    fn location(&self, id: &str, key: &str) -> Option<Location> {
        self.states.get(id)?.records.get(key).copied()
    }

    fn record(&mut self, id: &str, key: &str) -> Result<Option<String>, StoreError> {
        let Some(location) = self.location(id, key) else {
            return Ok(None);
        };
        match self.entry(location)? {
            Entry::Replace {
                state_id, records, ..
            } if state_id == id => records
                .iter()
                .find(|record| record.key == key)
                .map(|record| Some(record.value.clone()))
                .ok_or_else(|| backend("JSONL record index does not reference its record")),
            _ => Err(backend("JSONL record index does not reference its state")),
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

    fn append(&mut self, entry: Entry) -> Result<(), StoreError> {
        let span = tracing::info_span!(
            "durability.jsonl.append",
            log.bytes = tracing::field::Empty,
            encode.duration_ms = tracing::field::Empty,
            fsync.duration_ms = tracing::field::Empty,
            index.duration_ms = tracing::field::Empty
        );
        let _entered = span.enter();
        let started = Instant::now();
        let mut bytes = serde_json::to_vec(&entry).map_err(backend)?;
        bytes.push(b'\n');
        span.record("log.bytes", bytes.len());
        span.record(
            "encode.duration_ms",
            started.elapsed().as_secs_f64() * 1_000.0,
        );
        self.file
            .seek(SeekFrom::Start(self.offset))
            .map_err(backend)?;
        self.file.write_all(&bytes).map_err(backend)?;
        let started = Instant::now();
        self.file.sync_all().map_err(backend)?;
        span.record(
            "fsync.duration_ms",
            started.elapsed().as_secs_f64() * 1_000.0,
        );
        let started = Instant::now();
        let location = Location {
            position: self.offset,
            length: bytes.len(),
        };
        match entry {
            Entry::Acquire { state_id, owner } => self.acquired(&state_id, owner)?,
            Entry::Replace {
                state_id,
                owner,
                revision,
                records,
                ..
            } => {
                self.check_replacement(&state_id, &owner, revision)?;
                self.replaced(
                    &state_id,
                    revision,
                    location,
                    records.iter().map(|record| record.key.as_str()),
                );
            }
        }
        span.record(
            "index.duration_ms",
            started.elapsed().as_secs_f64() * 1_000.0,
        );
        self.offset = self
            .offset
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| backend("JSONL offset overflow"))?;
        Ok(())
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
            let mut grouped = HashMap::<Location, Vec<(usize, &String)>>::new();
            let mut values = vec![None; keys.len()];
            for (index, key) in keys.iter().enumerate() {
                if let Some(location) = store.location(id, key) {
                    grouped.entry(location).or_default().push((index, key));
                }
            }
            for (location, requested) in grouped {
                let Entry::Replace {
                    state_id, records, ..
                } = store.entry(location)?
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
        let result =
            self.locked(|store| Ok(store.created.iter().rev().take(limit).cloned().collect()));
        Box::pin(async move { result })
    }
    fn acquire<'a>(
        &'a mut self,
        id: &'a str,
        owner_id: OwnerId,
    ) -> StoreFuture<'a, Result<OwnedState, StoreError>> {
        let result = self.locked(|store| {
            let fence = store
                .next_fence(id)
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
            if store.owners.get(id) != Some(owner) {
                return Err(StoreError::Fenced);
            }
            let actual = store.revision(id);
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
fn identity(metadata: &Metadata) -> Result<String, StoreError> {
    use std::os::unix::fs::MetadataExt;
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn identity(metadata: &Metadata) -> Result<String, StoreError> {
    Ok(format!("{:?}", metadata.created().map_err(backend)?))
}

fn identity_changed() -> StoreError {
    backend("JSONL log identity changed while its store was open")
}

fn backend(error: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(error.to_string())
}

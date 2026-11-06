pub use crate::store_types::{IndexedRevision, TemporalMetadata, sha256};
use crate::{model::*, store_types::Record};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{cell::RefCell, collections::BTreeMap};

const MAX_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;
const MAX_OBJECT_BYTES: usize = 64 * 1024 * 1024;
const LAST_TIMESTAMP: &str = "9999-12-31T23:59:59.999999999Z";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: u32,
    sources: BTreeMap<String, Vec<u8>>,
    versions: BTreeMap<String, Vec<u8>>,
    records: BTreeMap<String, Vec<u8>>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            sources: BTreeMap::new(),
            versions: BTreeMap::new(),
            records: BTreeMap::new(),
        }
    }
}

#[derive(Default)]
pub struct Store {
    snapshot: RefCell<Snapshot>,
}

fn invalid(message: &str) -> HistoryError {
    HistoryError::Invalid(message.into())
}

fn object(objects: &BTreeMap<String, Vec<u8>>, digest: &str) -> Result<Vec<u8>> {
    objects
        .get(digest)
        .cloned()
        .ok_or_else(|| HistoryError::NotFound(format!("Missing object {digest}")))
}

fn put(objects: &mut BTreeMap<String, Vec<u8>>, bytes: &[u8]) -> Result<String> {
    if !law_domain::input_fits(bytes.len(), MAX_OBJECT_BYTES) {
        return Err(invalid("Object exceeds the 64 MiB input limit"));
    }
    let digest = sha256(bytes);
    objects
        .entry(digest.clone())
        .or_insert_with(|| bytes.to_vec());
    Ok(digest)
}

fn timestamp(date: DateTime<Utc>) -> String {
    date.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn visible(date: DateTime<Utc>, cutoff: Option<DateTime<Utc>>) -> bool {
    law_domain::knowledge_cutoff_accepts(
        timestamp(date).cmp(
            &cutoff
                .map(timestamp)
                .unwrap_or_else(|| LAST_TIMESTAMP.into()),
        ),
    )
}

impl Store {
    pub fn preserve(&self, bytes: &[u8]) -> Result<String> {
        put(&mut self.snapshot.borrow_mut().sources, bytes)
    }

    pub fn source(&self, digest: &str) -> Result<Vec<u8>> {
        object(&self.snapshot.borrow().sources, digest)
    }

    pub fn version(&self, digest: &str) -> Result<Revision> {
        let revision: Revision =
            serde_json::from_slice(&object(&self.snapshot.borrow().versions, digest)?)?;
        revision.validate().map_err(HistoryError::Invalid)?;
        for source in revision.sources() {
            self.source(&source.raw_sha256)?;
        }
        Ok(revision)
    }

    pub fn save_revision(&self, revision: &Revision, observed_at: DateTime<Utc>) -> Result<String> {
        self.validate_revision(revision)?;
        let version = put(
            &mut self.snapshot.borrow_mut().versions,
            &serde_json::to_vec(revision)?,
        )?;
        self.record(&Record::Observation(Observation {
            schema_version: SCHEMA_VERSION,
            version: version.clone(),
            observed_at,
        }))?;
        Ok(version)
    }

    fn validate_revision(&self, revision: &Revision) -> Result<()> {
        revision.validate().map_err(HistoryError::Invalid)?;
        for source in revision.sources() {
            self.source(&source.raw_sha256)?;
        }
        Ok(())
    }

    pub fn save_revisions(
        &self,
        revisions: &[Revision],
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<String>> {
        if revisions.is_empty() {
            return Err(invalid("Empty observation batch"));
        }
        for revision in revisions {
            self.validate_revision(revision)?;
        }
        let mut observations = Vec::new();
        for revision in revisions {
            observations.push(Observation {
                schema_version: SCHEMA_VERSION,
                version: put(
                    &mut self.snapshot.borrow_mut().versions,
                    &serde_json::to_vec(revision)?,
                )?,
                observed_at,
            });
        }
        let versions = observations.iter().map(|row| row.version.clone()).collect();
        self.record(&Record::Batch {
            schema_version: SCHEMA_VERSION,
            observations,
        })?;
        Ok(versions)
    }

    fn record(&self, record: &Record) -> Result<()> {
        put(
            &mut self.snapshot.borrow_mut().records,
            &serde_json::to_vec(record)?,
        )?;
        Ok(())
    }

    fn records(&self) -> Result<Vec<(String, Record)>> {
        self.snapshot
            .borrow()
            .records
            .iter()
            .map(|(digest, bytes)| Ok((digest.clone(), serde_json::from_slice(bytes)?)))
            .collect()
    }

    pub fn fact<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        observed_at: DateTime<Utc>,
    ) -> Result<()> {
        self.record(&Record::Fact {
            schema_version: SCHEMA_VERSION,
            key: key.into(),
            value: serde_json::to_value(value)?,
            observed_at,
        })
    }

    pub fn latest_fact<T: DeserializeOwned>(
        &self,
        key: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Option<T>> {
        let latest = self
            .records()?
            .into_iter()
            .filter_map(|(digest, record)| match record {
                Record::Fact {
                    key: name,
                    value,
                    observed_at,
                    ..
                } if name == key && visible(observed_at, known_at) => {
                    Some((timestamp(observed_at), digest, value))
                }
                _ => None,
            })
            .max_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
        latest
            .map(|(_, _, value)| serde_json::from_value(value).map_err(Into::into))
            .transpose()
    }

    pub fn fact_keys(&self, prefix: &str) -> Result<Vec<String>> {
        if prefix.contains('\0') {
            return Ok(Vec::new());
        }
        let mut keys = std::collections::BTreeSet::new();
        for (_, record) in self.records()? {
            if let Record::Fact { key, .. } = record
                && key.starts_with(prefix)
            {
                keys.insert(key);
            }
        }
        Ok(keys.into_iter().collect())
    }

    pub fn facts_with_prefix<T: DeserializeOwned>(
        &self,
        prefix: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<T>> {
        self.fact_keys(prefix)?
            .into_iter()
            .filter_map(|key| self.latest_fact(&key, known_at).transpose())
            .collect()
    }

    fn observations(
        &self,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<BTreeMap<String, DateTime<Utc>>> {
        let mut result: BTreeMap<String, DateTime<Utc>> = BTreeMap::new();
        for (_, record) in self.records()? {
            let observations = match record {
                Record::Observation(row) => vec![row],
                Record::Batch { observations, .. } => observations,
                Record::Fact { .. } => continue,
            };
            for row in observations {
                if visible(row.observed_at, known_at) {
                    result
                        .entry(row.version)
                        .and_modify(|date| {
                            *date = std::cmp::max_by_key(*date, row.observed_at, |value| {
                                timestamp(*value)
                            });
                        })
                        .or_insert(row.observed_at);
                }
            }
        }
        Ok(result)
    }

    pub fn indexed_history(
        &self,
        law_id: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<IndexedRevision>> {
        let mut result = Vec::new();
        for (version, observed_at) in self.observations(known_at)? {
            let revision = self.version(&version)?;
            if revision.law_id == law_id {
                result.push(IndexedRevision {
                    version,
                    observed_at,
                    metadata: TemporalMetadata::from(&revision),
                });
            }
        }
        result.sort_by(|a, b| {
            (timestamp(a.observed_at), &a.version).cmp(&(timestamp(b.observed_at), &b.version))
        });
        Ok(result)
    }

    pub fn history(
        &self,
        law_id: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<StoredRevision>> {
        let mut result: Vec<StoredRevision> = self
            .indexed_history(law_id, known_at)?
            .into_iter()
            .map(|row| {
                Ok(StoredRevision {
                    revision: self.version(&row.version)?,
                    version: row.version,
                    observed_at: row.observed_at,
                })
            })
            .collect::<Result<_>>()?;
        result.sort_by(|a, b| {
            (
                a.revision.effective.day(),
                a.revision.sequence,
                &a.revision.revision_id,
                a.observed_at,
            )
                .cmp(&(
                    b.revision.effective.day(),
                    b.revision.sequence,
                    &b.revision.revision_id,
                    b.observed_at,
                ))
        });
        Ok(result)
    }

    pub fn law_ids(&self) -> Result<Vec<String>> {
        let mut ids = std::collections::BTreeSet::new();
        for (_, record) in self.records()? {
            let observations = match record {
                Record::Observation(row) => vec![row],
                Record::Batch { observations, .. } => observations,
                Record::Fact { .. } => continue,
            };
            for row in observations {
                ids.insert(self.version(&row.version)?.law_id);
            }
        }
        Ok(ids.into_iter().collect())
    }

    pub fn export(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(&*self.snapshot.borrow())?;
        if !law_domain::input_fits(bytes.len(), MAX_SNAPSHOT_BYTES) {
            return Err(invalid("Snapshot exceeds the 256 MiB input limit"));
        }
        Ok(bytes)
    }

    pub fn restore(&self, bytes: &[u8]) -> Result<()> {
        if !law_domain::input_fits(bytes.len(), MAX_SNAPSHOT_BYTES) {
            return Err(invalid("Snapshot exceeds the 256 MiB input limit"));
        }
        let snapshot: Snapshot = serde_json::from_slice(bytes)?;
        if snapshot.schema_version != SCHEMA_VERSION {
            return Err(invalid("Unsupported snapshot schema"));
        }
        for objects in [&snapshot.sources, &snapshot.versions, &snapshot.records] {
            for (digest, bytes) in objects {
                if !law_domain::input_fits(bytes.len(), MAX_OBJECT_BYTES)
                    || sha256(bytes) != *digest
                {
                    return Err(invalid("Snapshot object integrity failure"));
                }
            }
        }
        let candidate = Self {
            snapshot: RefCell::new(snapshot),
        };
        for digest in candidate.snapshot.borrow().versions.keys() {
            candidate.version(digest)?;
        }
        for (_, record) in candidate.records()? {
            match record {
                Record::Fact { schema_version, .. } if schema_version == SCHEMA_VERSION => {}
                Record::Observation(row) => candidate.validate_observation(&row)?,
                Record::Batch {
                    schema_version,
                    observations,
                } if schema_version == SCHEMA_VERSION && !observations.is_empty() => {
                    for row in observations {
                        candidate.validate_observation(&row)?;
                    }
                }
                _ => return Err(invalid("Unsupported or empty snapshot record")),
            }
        }
        *self.snapshot.borrow_mut() = candidate.snapshot.into_inner();
        Ok(())
    }

    fn validate_observation(&self, row: &Observation) -> Result<()> {
        if row.schema_version != SCHEMA_VERSION {
            return Err(invalid("Unsupported observation schema"));
        }
        self.version(&row.version)?;
        Ok(())
    }
}

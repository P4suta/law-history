use crate::{codec, model::*};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const INDEX_BUSY_TIMEOUT: Duration = Duration::from_secs(10);
const SHA256_HEX_LENGTH: usize = 64;

use crate::store_types::Record;
pub use crate::store_types::{IndexedRevision, TemporalMetadata, sha256};

pub struct Store {
    root: PathBuf,
    db: Connection,
}

#[derive(Debug, Serialize)]
pub struct CompactReport {
    pub dry_run: bool,
    pub objects: usize,
    pub compacted_objects: usize,
    pub object_bytes_before: u64,
    pub object_bytes_after: u64,
    pub index_bytes_before: u64,
    pub index_bytes_after: Option<u64>,
}

impl Store {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        for directory in ["sources", "versions", "records"] {
            fs::create_dir_all(root.join(directory))?;
        }
        let db = Connection::open(root.join("index.sqlite3"))?;
        db.busy_timeout(INDEX_BUSY_TIMEOUT)?;
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS revisions(version TEXT PRIMARY KEY, law_id TEXT NOT NULL, revision_id TEXT NOT NULL, title TEXT NOT NULL, json TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS law_revisions ON revisions(law_id);
            CREATE TABLE IF NOT EXISTS observations(version TEXT NOT NULL, observed_at TEXT NOT NULL, PRIMARY KEY(version, observed_at));
            CREATE TABLE IF NOT EXISTS facts(key TEXT NOT NULL, observed_at TEXT NOT NULL, digest TEXT NOT NULL, json TEXT NOT NULL, PRIMARY KEY(key, observed_at, digest));")?;
        Ok(Self { root, db })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn put_object(&self, category: &str, bytes: &[u8]) -> Result<String> {
        let digest = sha256(bytes);
        if self.root.join(category).join(&digest).exists()
            || self
                .root
                .join(category)
                .join(format!("{digest}.zst"))
                .exists()
        {
            require_identical_bytes(
                &self.get_object(category, &digest)?,
                bytes,
                "Existing content address has different bytes",
            )?;
            return Ok(digest);
        }
        let encoded = codec::encode(bytes)?;
        let compressed = encoded.len() < bytes.len();
        let destination = self.root.join(category).join(if compressed {
            format!("{digest}.zst")
        } else {
            digest.clone()
        });
        let stored = if compressed {
            encoded.as_slice()
        } else {
            bytes
        };
        let mut temporary = tempfile::NamedTempFile::new_in(self.root.join(category))?;
        temporary.write_all(stored)?;
        temporary.as_file().sync_all()?;
        allow_existing_publication(
            temporary
                .persist_noclobber(&destination)
                .map(|_| ())
                .map_err(|error| error.error),
        )?;
        require_identical_bytes(
            &self.get_object(category, &digest)?,
            bytes,
            "Published object integrity failure",
        )?;
        Ok(digest)
    }

    fn get_object(&self, category: &str, digest: &str) -> Result<Vec<u8>> {
        if digest.len() != SHA256_HEX_LENGTH
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(HistoryError::Invalid("Invalid object digest".into()));
        }
        let raw = self.root.join(category).join(digest);
        let compressed = self.root.join(category).join(format!("{digest}.zst"));
        let bytes = match read_bounded(&raw) {
            Ok(bytes) => {
                match read_bounded(&compressed) {
                    Ok(encoded) if codec::decode(&encoded, codec::MAX_OBJECT_BYTES)? != bytes => {
                        return Err(HistoryError::Invalid(
                            "Raw and compressed objects disagree".into(),
                        ));
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                bytes
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                codec::decode(&read_bounded(&compressed)?, codec::MAX_OBJECT_BYTES)?
            }
            Err(error) => return Err(error.into()),
        };
        if sha256(&bytes) != digest {
            return Err(HistoryError::Invalid(format!(
                "Corrupt {category} object {digest}"
            )));
        }
        Ok(bytes)
    }

    pub fn preserve(&self, bytes: &[u8]) -> Result<String> {
        self.put_object("sources", bytes)
    }
    pub fn source(&self, digest: &str) -> Result<Vec<u8>> {
        self.get_object("sources", digest)
    }

    fn verify_revision_sources(&self, revision: &Revision) -> Result<()> {
        for source in revision.sources() {
            self.source(&source.raw_sha256)?;
        }
        Ok(())
    }

    pub fn save_revision(&self, revision: &Revision, observed_at: DateTime<Utc>) -> Result<String> {
        revision.validate().map_err(HistoryError::Invalid)?;
        self.verify_revision_sources(revision)?;
        let bytes = serde_json::to_vec(revision)?;
        let version = self.put_object("versions", &bytes)?;
        let record = Record::Observation(Observation {
            schema_version: SCHEMA_VERSION,
            version: version.clone(),
            observed_at,
        });
        self.save_record(&record)?;
        Ok(version)
    }

    pub fn version(&self, digest: &str) -> Result<Revision> {
        let revision: Revision = serde_json::from_slice(&self.get_object("versions", digest)?)?;
        revision.validate().map_err(HistoryError::Invalid)?;
        self.verify_revision_sources(&revision)?;
        Ok(revision)
    }

    fn save_record(&self, record: &Record) -> Result<()> {
        let bytes = serde_json::to_vec(record)?;
        let digest = self.put_object("records", &bytes)?;
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        match self.index(record, &digest) {
            Ok(()) => {
                self.db.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(error) => {
                self.db.execute_batch("ROLLBACK")?;
                Err(error)
            }
        }
    }

    pub fn save_revisions(
        &self,
        revisions: &[Revision],
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<String>> {
        for revision in revisions {
            revision.validate().map_err(HistoryError::Invalid)?;
            self.verify_revision_sources(revision)?;
        }
        let mut observations = Vec::new();
        for revision in revisions {
            let version = self.put_object("versions", &serde_json::to_vec(revision)?)?;
            observations.push(Observation {
                schema_version: SCHEMA_VERSION,
                version,
                observed_at,
            });
        }
        let versions = observations
            .iter()
            .map(|observation| observation.version.clone())
            .collect();
        self.save_record(&Record::Batch {
            schema_version: SCHEMA_VERSION,
            observations,
        })?;
        Ok(versions)
    }

    fn index(&self, record: &Record, digest: &str) -> Result<()> {
        match record {
            Record::Batch {
                schema_version,
                observations,
            } => {
                if *schema_version != SCHEMA_VERSION || observations.is_empty() {
                    return Err(HistoryError::Invalid(
                        "Unsupported or empty observation batch".into(),
                    ));
                }
                for observation in observations {
                    self.index(&Record::Observation(observation.clone()), digest)?;
                }
            }
            Record::Observation(observation) => {
                if observation.schema_version != SCHEMA_VERSION {
                    return Err(HistoryError::Invalid(
                        "Unsupported observation schema".into(),
                    ));
                }
                let revision = self.version(&observation.version)?;
                self.db.execute(
                    "INSERT OR IGNORE INTO revisions VALUES(?1,?2,?3,?4,?5)",
                    params![
                        observation.version,
                        revision.law_id,
                        revision.revision_id,
                        revision.title,
                        serde_json::to_string(&revision_metadata(&revision)?)?
                    ],
                )?;
                self.db.execute(
                    "INSERT OR IGNORE INTO observations VALUES(?1,?2)",
                    params![
                        observation.version,
                        observation
                            .observed_at
                            .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
                    ],
                )?;
            }
            Record::Fact {
                schema_version,
                key,
                value,
                observed_at,
            } => {
                if *schema_version != SCHEMA_VERSION {
                    return Err(HistoryError::Invalid("Unsupported fact schema".into()));
                }
                self.db.execute(
                    "INSERT OR IGNORE INTO facts VALUES(?1,?2,?3,?4)",
                    params![
                        key,
                        observed_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
                        digest,
                        serde_json::to_string(value)?
                    ],
                )?;
            }
        }
        Ok(())
    }

    pub fn fact<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        observed_at: DateTime<Utc>,
    ) -> Result<()> {
        self.save_record(&Record::Fact {
            schema_version: SCHEMA_VERSION,
            key: key.into(),
            value: serde_json::to_value(value)?,
            observed_at,
        })
    }

    pub fn latest_fact<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Option<T>> {
        let cutoff = known_at
            .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
            .unwrap_or_else(|| "9999-12-31T23:59:59.999999999Z".into());
        let mut query = self.db.prepare("SELECT json FROM facts WHERE key=?1 AND observed_at<=?2 ORDER BY observed_at DESC,digest DESC LIMIT 1")?;
        let mut rows = query.query(params![key, cutoff])?;
        rows.next()?
            .map(|row| Ok(serde_json::from_str(&row.get::<_, String>(0)?)?))
            .transpose()
    }

    pub fn facts_with_prefix<T: serde::de::DeserializeOwned>(
        &self,
        prefix: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<T>> {
        self.fact_keys(prefix)?
            .into_iter()
            .filter_map(|key| self.latest_fact(&key, known_at).transpose())
            .collect()
    }

    pub fn fact_keys(&self, prefix: &str) -> Result<Vec<String>> {
        let mut statement = self
            .db
            .prepare("SELECT DISTINCT key FROM facts WHERE substr(key,1,?1)=?2 ORDER BY key")?;
        let keys = statement
            .query_map(params![prefix.chars().count(), prefix], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(keys)
    }

    pub fn history(
        &self,
        law_id: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<StoredRevision>> {
        let cutoff = known_at
            .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
            .unwrap_or_else(|| "9999-12-31T23:59:59.999999999Z".into());
        let mut statement = self.db.prepare("SELECT r.version,r.json,MAX(o.observed_at) FROM revisions r JOIN observations o ON o.version=r.version WHERE r.law_id=?1 AND o.observed_at<=?2 GROUP BY r.version ORDER BY MAX(o.observed_at),r.version")?;
        let mut rows = statement.query(params![law_id, cutoff])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let version: String = row.get(0)?;
            let revision = self.version(&version)?;
            if revision.law_id != law_id {
                return Err(HistoryError::Invalid(
                    "The search index points to a different law; rebuild the index".into(),
                ));
            }
            result.push(StoredRevision {
                version,
                revision,
                observed_at: row
                    .get::<_, String>(2)?
                    .parse()
                    .map_err(|_| HistoryError::Invalid("Corrupt observation timestamp".into()))?,
            });
        }
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

    pub fn indexed_history(
        &self,
        law_id: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<IndexedRevision>> {
        let cutoff = known_at
            .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
            .unwrap_or_else(|| "9999-12-31T23:59:59.999999999Z".into());
        let mut statement = self.db.prepare("SELECT r.version,json_remove(r.json,'$.tree'),MAX(o.observed_at) FROM revisions r JOIN observations o ON o.version=r.version WHERE r.law_id=?1 AND o.observed_at<=?2 GROUP BY r.version ORDER BY MAX(o.observed_at),r.version")?;
        let mut rows = statement.query(params![law_id, cutoff])?;
        let mut revisions = Vec::new();
        while let Some(row) = rows.next()? {
            let metadata: TemporalMetadata = serde_json::from_str(&row.get::<_, String>(1)?)?;
            if metadata.law_id != law_id {
                return Err(HistoryError::Invalid(
                    "Indexed law identity is inconsistent; rebuild the index".into(),
                ));
            }
            revisions.push(IndexedRevision {
                version: row.get(0)?,
                metadata,
                observed_at: row
                    .get::<_, String>(2)?
                    .parse()
                    .map_err(|_| HistoryError::Invalid("Corrupt observation timestamp".into()))?,
            });
        }
        Ok(revisions)
    }

    pub fn law_ids(&self) -> Result<Vec<String>> {
        let mut statement = self
            .db
            .prepare("SELECT DISTINCT law_id FROM revisions ORDER BY law_id")?;
        Ok(statement
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn rebuild(&self) -> Result<usize> {
        let records = self.object_digests("records")?;
        self.db.execute_batch(
            "BEGIN IMMEDIATE; DELETE FROM facts; DELETE FROM observations; DELETE FROM revisions;",
        )?;
        let operation = (|| {
            let mut count = 0;
            for name in records {
                let bytes = self.get_object("records", &name)?;
                let record = serde_json::from_slice(&bytes)?;
                self.index(&record, &name)?;
                count += 1;
            }
            Ok(count)
        })();
        match operation {
            Ok(count) => {
                self.db.execute_batch("COMMIT")?;
                Ok(count)
            }
            Err(error) => {
                self.db.execute_batch("ROLLBACK")?;
                Err(error)
            }
        }
    }

    pub fn verify_integrity(&self) -> Result<usize> {
        let mut count = 0;
        for category in ["sources", "versions", "records"] {
            for name in self.object_digests(category)? {
                self.get_object(category, &name)?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn compact(&self, dry_run: bool) -> Result<CompactReport> {
        let index_size = || -> Result<u64> {
            ["index.sqlite3", "index.sqlite3-wal", "index.sqlite3-shm"]
                .iter()
                .try_fold(0, |total, name| {
                    Ok(total + file_size(&self.root.join(name))?)
                })
        };
        let mut report = CompactReport {
            dry_run,
            objects: 0,
            compacted_objects: 0,
            object_bytes_before: 0,
            object_bytes_after: 0,
            index_bytes_before: index_size()?,
            index_bytes_after: None,
        };
        for category in ["sources", "versions", "records"] {
            for digest in self.object_digests(category)? {
                let raw = self.root.join(category).join(&digest);
                let compressed = self.root.join(category).join(format!("{digest}.zst"));
                let raw_size = file_size(&raw)?;
                let compressed_size = file_size(&compressed)?;
                let bytes = self.get_object(category, &digest)?;
                let mut after = raw_size + compressed_size;
                if raw.exists() {
                    let encoded = codec::encode(&bytes)?;
                    if encoded.len() < bytes.len() {
                        after = if compressed.exists() {
                            compressed_size
                        } else {
                            encoded.len() as u64
                        };
                        if !dry_run {
                            let mut temporary =
                                tempfile::NamedTempFile::new_in(self.root.join(category))?;
                            temporary.write_all(&encoded)?;
                            temporary.as_file().sync_all()?;
                            allow_existing_publication(
                                temporary
                                    .persist_noclobber(&compressed)
                                    .map(|_| ())
                                    .map_err(|error| error.error),
                            )?;
                            require_identical_bytes(
                                &self.get_object(category, &digest)?,
                                &bytes,
                                "Compaction changed source bytes",
                            )?;
                            fs::remove_file(&raw)?;
                        }
                        report.compacted_objects += 1;
                    }
                }
                report.objects += 1;
                report.object_bytes_before += raw_size + compressed_size;
                report.object_bytes_after += after;
            }
        }
        if !dry_run {
            self.db.execute("UPDATE revisions SET json=json_remove(json,'$.tree') WHERE json_type(json,'$.tree') IS NOT NULL", [])?;
            self.db.execute_batch(
                "PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);",
            )?;
            report.index_bytes_after = Some(index_size()?);
        }
        Ok(report)
    }

    fn object_digests(&self, category: &str) -> Result<std::collections::BTreeSet<String>> {
        let mut digests = std::collections::BTreeSet::new();
        for entry in fs::read_dir(self.root.join(category))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if !entry.file_type()?.is_file() {
                return Err(HistoryError::Invalid(
                    "Object directories can contain only files".into(),
                ));
            }
            digests.insert(name.strip_suffix(".zst").unwrap_or(&name).to_owned());
        }
        Ok(digests)
    }
}

fn revision_metadata(revision: &Revision) -> Result<Value> {
    let mut value = serde_json::to_value(revision)?;
    value
        .as_object_mut()
        .ok_or_else(|| HistoryError::Invalid("A revision must serialize as an object".into()))?
        .remove("tree");
    Ok(value)
}

fn allow_existing_publication(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => Err(error),
        _ => Ok(()),
    }
}

fn require_identical_bytes(actual: &[u8], expected: &[u8], message: &str) -> Result<()> {
    if actual != expected {
        return Err(HistoryError::Invalid(message.into()));
    }
    Ok(())
}

fn file_size(path: &Path) -> Result<u64> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(codec::MAX_OBJECT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > codec::MAX_OBJECT_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Stored object exceeds the size limit",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_classification_accepts_only_creation_or_an_existing_destination() {
        assert!(allow_existing_publication(Ok(())).is_ok());
        assert!(allow_existing_publication(Err(std::io::ErrorKind::AlreadyExists.into())).is_ok());
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::InvalidInput,
            std::io::ErrorKind::StorageFull,
            std::io::ErrorKind::Other,
        ] {
            let error =
                allow_existing_publication(Err(std::io::Error::new(kind, "Publication failed")))
                    .unwrap_err();
            assert_eq!(error.kind(), kind);
            assert_eq!(error.to_string(), "Publication failed");
        }
    }

    #[test]
    fn concurrent_publication_preserves_one_verified_object_per_source() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        for round in 0..4 {
            let mut bytes = vec![b'x'; 1_048_576];
            bytes[0] = round;
            let expected = sha256(&bytes);
            let bytes = std::sync::Arc::new(bytes);
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
            let stores: Vec<_> = (0..8)
                .map(|_| Store::open(directory.path()).unwrap())
                .collect();
            std::thread::scope(|threads| {
                let handles: Vec<_> = stores
                    .into_iter()
                    .map(|store| {
                        let bytes = bytes.clone();
                        let barrier = barrier.clone();
                        threads.spawn(move || {
                            barrier.wait();
                            store.preserve(&bytes).unwrap()
                        })
                    })
                    .collect();
                for handle in handles {
                    assert_eq!(handle.join().unwrap(), expected);
                }
            });
            assert!(
                store.source(&expected).unwrap() == *bytes,
                "Concurrent writers must preserve all source bytes"
            );
        }
        assert_eq!(store.object_digests("sources").unwrap().len(), 4);
        assert_eq!(store.verify_integrity().unwrap(), 4);
    }

    #[test]
    fn byte_comparison_and_filesystem_failures_keep_their_error_causes() {
        assert!(require_identical_bytes(b"same", b"same", "context").is_ok());
        assert!(
            require_identical_bytes(b"other", b"same", "context")
                .unwrap_err()
                .to_string()
                .contains("context")
        );
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("file");
        fs::write(&file, "bytes").unwrap();
        assert!(file_size(&file.join("child")).is_err());
        assert_eq!(file_size(&directory.path().join("absent")).unwrap(), 0);
    }

    #[test]
    fn raw_object_reads_include_the_size_boundary_and_reject_larger_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("object");
        let file = fs::File::create(&path).unwrap();
        file.set_len(268_435_456).unwrap();
        assert_eq!(read_bounded(&path).unwrap().len(), 268_435_456);
        for size in [268_435_457, 268_435_458] {
            file.set_len(size).unwrap();
            assert!(read_bounded(&path).is_err());
        }
    }

    #[test]
    fn dual_representations_and_unreadable_paths_never_hide_corruption() {
        for compressed in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let store = Store::open(directory.path()).unwrap();
            let bytes = if compressed {
                vec![b'a'; 4096]
            } else {
                vec![b'x']
            };
            let digest = store.preserve(&bytes).unwrap();
            let other = directory.path().join("sources").join(if compressed {
                digest.clone()
            } else {
                format!("{digest}.zst")
            });
            fs::create_dir(&other).unwrap();
            assert!(store.source(&digest).is_err());
        }
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let digest = store.preserve(b"raw").unwrap();
        fs::write(
            directory
                .path()
                .join("sources")
                .join(format!("{digest}.zst")),
            codec::encode(b"different").unwrap(),
        )
        .unwrap();
        assert!(
            store
                .source(&digest)
                .unwrap_err()
                .to_string()
                .contains("disagree")
        );
        for digest in [
            "a".repeat(63),
            "g".repeat(64),
            "A".repeat(64),
            "a".repeat(65),
        ] {
            assert!(
                store
                    .source(&digest)
                    .unwrap_err()
                    .to_string()
                    .contains("Invalid object digest")
            );
        }
    }

    #[test]
    fn compaction_reports_actual_files_and_reuses_legacy_objects_without_transcoding() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let source = directory.path().join("sources");
        let equal = (0..512)
            .map(|size| vec![b'q'; size])
            .find(|bytes| codec::encode(bytes).unwrap().len() == bytes.len())
            .expect("Pinned codec equality boundary");
        let raw_large = vec![b'r'; 4096];
        let dual = vec![b'd'; 4096];
        for bytes in [&raw_large, &dual, &equal, &vec![b'x']] {
            fs::write(source.join(sha256(bytes)), bytes).unwrap();
        }
        let fresh = tempfile::tempdir().unwrap();
        let fresh_store = Store::open(fresh.path()).unwrap();
        let digest = fresh_store.preserve(&equal).unwrap();
        assert!(fresh.path().join("sources").join(&digest).is_file());
        assert!(
            !fresh
                .path()
                .join("sources")
                .join(format!("{digest}.zst"))
                .exists()
        );
        assert_eq!(fresh_store.source(&digest).unwrap(), equal);
        let legacy = sha256(&raw_large);
        assert_eq!(store.preserve(&raw_large).unwrap(), legacy);
        assert!(!source.join(format!("{legacy}.zst")).exists());
        fs::write(
            source.join(format!("{}.zst", sha256(&dual))),
            codec::encode(&dual).unwrap(),
        )
        .unwrap();
        let compressed = store.preserve(&vec![b'z'; 4096]).unwrap();
        assert!(source.join(format!("{compressed}.zst")).exists());
        assert!(!source.join(&compressed).exists());
        let total = || {
            fs::read_dir(&source)
                .unwrap()
                .map(|entry| entry.unwrap().metadata().unwrap().len())
                .sum::<u64>()
        };
        let before = total();
        let preview = store.compact(true).unwrap();
        assert_eq!(preview.objects, 5);
        assert_eq!(preview.compacted_objects, 2);
        assert_eq!(preview.object_bytes_before, before);
        assert_eq!(total(), before);
        assert!(preview.index_bytes_before > 0);
        assert!(preview.index_bytes_after.is_none());
        let result = store.compact(false).unwrap();
        assert_eq!(result.objects, 5);
        assert_eq!(result.compacted_objects, 2);
        assert_eq!(result.object_bytes_before, before);
        assert_eq!(result.object_bytes_after, total());
        assert_eq!(preview.object_bytes_after, result.object_bytes_after);
        assert!(result.index_bytes_after.is_some_and(|bytes| bytes > 0));
        assert!(!source.join(&legacy).exists());
        assert!(source.join(format!("{legacy}.zst")).exists());
        assert!(source.join(sha256(&equal)).exists());
        assert!(!source.join(format!("{}.zst", sha256(&equal))).exists());
        for bytes in [&raw_large, &dual, &equal] {
            assert_eq!(store.source(&sha256(bytes)).unwrap(), *bytes);
        }
    }

    #[test]
    fn observation_batches_refuse_unsupported_schema_and_empty_members_independently() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        crate::ingest::import_bundle(&store, include_bytes!("../../../fixtures/demo.json"))
            .unwrap();
        let version = store.history("DEMO_ACT", None).unwrap()[0].version.clone();
        let observation = Observation {
            schema_version: SCHEMA_VERSION,
            version,
            observed_at: "2026-11-05T00:00:00Z".parse().unwrap(),
        };
        for (schema_version, observations) in [(0, vec![observation]), (SCHEMA_VERSION, Vec::new())]
        {
            let record = Record::Batch {
                schema_version,
                observations,
            };
            let digest = sha256(&serde_json::to_vec(&record).unwrap());
            assert!(store.index(&record, &digest).is_err());
        }
    }
}

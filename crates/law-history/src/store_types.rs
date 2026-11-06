use crate::model::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Record {
    Observation(Observation),
    Batch {
        schema_version: u32,
        observations: Vec<Observation>,
    },
    Fact {
        schema_version: u32,
        key: String,
        value: Value,
        observed_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TemporalMetadata {
    pub law_id: String,
    pub revision_id: String,
    pub promulgated: LegalDate,
    #[serde(default)]
    pub amendment_promulgated: Option<LegalDate>,
    pub effective: LegalDate,
    pub repeal: Option<Day>,
    pub termination: Option<Termination>,
    pub sequence: Option<u32>,
    pub original_enactment: bool,
    pub source: Provenance,
    #[serde(default)]
    pub metadata_source: Option<Provenance>,
}

impl From<&Revision> for TemporalMetadata {
    fn from(revision: &Revision) -> Self {
        Self {
            law_id: revision.law_id.clone(),
            revision_id: revision.revision_id.clone(),
            promulgated: revision.promulgated.clone(),
            amendment_promulgated: revision.amendment_promulgated.clone(),
            effective: revision.effective.clone(),
            repeal: revision.repeal,
            termination: revision.termination.clone(),
            sequence: revision.sequence,
            original_enactment: revision.original_enactment,
            source: revision.source.clone(),
            metadata_source: revision.metadata_source.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedRevision {
    pub version: String,
    pub observed_at: DateTime<Utc>,
    pub metadata: TemporalMetadata,
}

pub trait Repository {
    fn preserve(&self, bytes: &[u8]) -> Result<String>;
    fn source(&self, digest: &str) -> Result<Vec<u8>>;
    fn version(&self, digest: &str) -> Result<Revision>;
    fn save_revision(&self, revision: &Revision, observed_at: DateTime<Utc>) -> Result<String>;
    fn save_revisions(
        &self,
        revisions: &[Revision],
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<String>>;
    fn fact<T: Serialize>(&self, key: &str, value: &T, observed_at: DateTime<Utc>) -> Result<()>;
    fn latest_fact<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Option<T>>;
    fn facts_with_prefix<T: serde::de::DeserializeOwned>(
        &self,
        prefix: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<T>>;
    fn fact_keys(&self, prefix: &str) -> Result<Vec<String>>;
    fn history(&self, law_id: &str, known_at: Option<DateTime<Utc>>)
    -> Result<Vec<StoredRevision>>;
    fn indexed_history(
        &self,
        law_id: &str,
        known_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<IndexedRevision>>;
    fn law_ids(&self) -> Result<Vec<String>>;
}

macro_rules! repository_impl {
    ($store:ty) => {
        impl Repository for $store {
            fn preserve(&self, bytes: &[u8]) -> Result<String> {
                self.preserve(bytes)
            }
            fn source(&self, digest: &str) -> Result<Vec<u8>> {
                self.source(digest)
            }
            fn version(&self, digest: &str) -> Result<Revision> {
                self.version(digest)
            }
            fn save_revision(
                &self,
                revision: &Revision,
                observed_at: DateTime<Utc>,
            ) -> Result<String> {
                self.save_revision(revision, observed_at)
            }
            fn save_revisions(
                &self,
                revisions: &[Revision],
                observed_at: DateTime<Utc>,
            ) -> Result<Vec<String>> {
                self.save_revisions(revisions, observed_at)
            }
            fn fact<T: Serialize>(
                &self,
                key: &str,
                value: &T,
                observed_at: DateTime<Utc>,
            ) -> Result<()> {
                self.fact(key, value, observed_at)
            }
            fn latest_fact<T: serde::de::DeserializeOwned>(
                &self,
                key: &str,
                known_at: Option<DateTime<Utc>>,
            ) -> Result<Option<T>> {
                self.latest_fact(key, known_at)
            }
            fn facts_with_prefix<T: serde::de::DeserializeOwned>(
                &self,
                prefix: &str,
                known_at: Option<DateTime<Utc>>,
            ) -> Result<Vec<T>> {
                self.facts_with_prefix(prefix, known_at)
            }
            fn fact_keys(&self, prefix: &str) -> Result<Vec<String>> {
                self.fact_keys(prefix)
            }
            fn history(
                &self,
                law_id: &str,
                known_at: Option<DateTime<Utc>>,
            ) -> Result<Vec<StoredRevision>> {
                self.history(law_id, known_at)
            }
            fn indexed_history(
                &self,
                law_id: &str,
                known_at: Option<DateTime<Utc>>,
            ) -> Result<Vec<IndexedRevision>> {
                self.indexed_history(law_id, known_at)
            }
            fn law_ids(&self) -> Result<Vec<String>> {
                self.law_ids()
            }
        }
    };
}

repository_impl!(crate::memory_store::Store);
#[cfg(not(target_arch = "wasm32"))]
repository_impl!(crate::store::Store);

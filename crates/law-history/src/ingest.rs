#[cfg(not(target_arch = "wasm32"))]
use crate::{Store, store::sha256};
use crate::{model::*, xml::parse_xml};
use anyhow::{Context, bail};
use base64::Engine;
use chrono::{DateTime, Utc};
#[cfg(not(target_arch = "wasm32"))]
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[cfg(not(target_arch = "wasm32"))]
use std::{io::Read, time::Duration};
use utoipa::ToSchema;

pub const EGOV_API: &str = "https://laws.e-gov.go.jp/api/2";
pub const EGOV_TERMS: &str = "https://laws.e-gov.go.jp/terms";
#[cfg(not(target_arch = "wasm32"))]
const EXTRACTOR_VERSION: u32 = 7;
#[cfg(not(target_arch = "wasm32"))]
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LawInfo {
    pub law_id: String,
    pub law_num: String,
    pub promulgation_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RevisionInfo {
    #[serde(flatten)]
    pub metadata: RevisionMetadata,
    pub updated: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum SourceRepealStatus {
    None,
    Repeal,
    Expire,
    Suspend,
    LossOfEffectiveness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RevisionMetadata {
    pub law_revision_id: String,
    pub law_title: String,
    pub amendment_promulgate_date: Option<String>,
    pub amendment_enforcement_date: Option<String>,
    pub amendment_enforcement_comment: Option<String>,
    pub amendment_scheduled_enforcement_date: Option<String>,
    pub amendment_law_id: Option<String>,
    pub amendment_type: String,
    pub mission: String,
    pub repeal_status: SourceRepealStatus,
    pub repeal_date: Option<String>,
    pub current_revision_status: String,
}

impl RevisionMetadata {
    pub fn agrees_with(&self, other: &Self) -> bool {
        self.merged(other).is_ok()
    }

    pub fn merged(&self, other: &Self) -> anyhow::Result<Self> {
        let scheduled: Option<String> = serde_json::from_value(merge_value(
            serde_json::to_value(&self.amendment_scheduled_enforcement_date)?,
            serde_json::to_value(&other.amendment_scheduled_enforcement_date)?,
            "/revision_info/amendment_scheduled_enforcement_date",
        )?)?;
        let comparable = |metadata: &RevisionMetadata| {
            let mut result = metadata.clone();
            result.law_title = crate::xml::title_text(&metadata.law_title)
                .map_err(anyhow::Error::msg)
                .context("Invalid inline title at /revision_info/law_title")?;
            result.amendment_scheduled_enforcement_date = scheduled.clone();
            let actual = metadata
                .amendment_enforcement_date
                .as_deref()
                .filter(|value| !value.trim().is_empty());
            if law_domain::enforcement_date_role(actual.is_some(), actual == scheduled.as_deref())
                != law_domain::EnforcementDateRole::DistinctValue
            {
                result.amendment_enforcement_date = None;
            }
            Ok::<_, anyhow::Error>(result)
        };
        let first = comparable(self)?;
        let second = comparable(other)?;
        let raw_title = if first.law_title.trim().is_empty() {
            &other.law_title
        } else {
            &self.law_title
        };
        let mut merged: Self = merge_metadata(&first, &second, "/revision_info")?;
        merged.law_title = raw_title.clone();
        Ok(merged)
    }
}

fn merge_value(
    first: serde_json::Value,
    second: serde_json::Value,
    path: &str,
) -> anyhow::Result<serde_json::Value> {
    fn known(value: serde_json::Value) -> serde_json::Value {
        if value.as_str().is_some_and(|text| text.trim().is_empty()) {
            serde_json::Value::Null
        } else {
            value
        }
    }
    let first = known(first);
    let second = known(second);
    match law_domain::evidence_choice(!first.is_null(), !second.is_null(), first == second) {
        law_domain::EvidenceChoice::Absent => Ok(serde_json::Value::Null),
        law_domain::EvidenceChoice::First => Ok(first),
        law_domain::EvidenceChoice::Second => Ok(second),
        law_domain::EvidenceChoice::Conflict => {
            Err(MetadataDisagreement { path: path.into() }.into())
        }
    }
}

fn merge_metadata<T: Serialize + serde::de::DeserializeOwned>(
    first: &T,
    second: &T,
    path: &str,
) -> anyhow::Result<T> {
    let serde_json::Value::Object(first) = serde_json::to_value(first)? else {
        bail!("Expected object-shaped source metadata");
    };
    let serde_json::Value::Object(second) = serde_json::to_value(second)? else {
        bail!("Expected object-shaped source metadata");
    };
    let names: BTreeSet<_> = first.keys().chain(second.keys()).collect();
    let mut merged = serde_json::Map::new();
    for name in names {
        merged.insert(
            name.clone(),
            merge_value(
                first.get(name).cloned().unwrap_or_default(),
                second.get(name).cloned().unwrap_or_default(),
                &format!("{path}/{name}"),
            )?,
        );
    }
    Ok(serde_json::from_value(serde_json::Value::Object(merged))?)
}

#[derive(Debug, Deserialize)]
pub(crate) struct RevisionsResponse {
    pub(crate) law_info: LawInfo,
    pub(crate) revisions: Vec<RevisionInfo>,
}

pub struct HistorySnapshot {
    pub(crate) response: RevisionsResponse,
    source: Provenance,
}

pub struct RevisionExpectation<'a> {
    law: &'a LawInfo,
    info: &'a RevisionInfo,
    source: Provenance,
}

impl HistorySnapshot {
    pub fn from_response(
        store: &impl crate::store_types::Repository,
        bytes: &[u8],
        url: &str,
    ) -> anyhow::Result<Self> {
        let response: RevisionsResponse = serde_json::from_slice(bytes)?;
        validate_id(&response.law_info.law_id).map_err(anyhow::Error::msg)?;
        for info in &response.revisions {
            validate_id(&info.metadata.law_revision_id).map_err(anyhow::Error::msg)?;
        }
        Ok(Self {
            response,
            source: Provenance {
                url: url.into(),
                raw_sha256: store.preserve(bytes)?,
                media_type: "application/json".into(),
                attribution: "e-Gov Law Search, Digital Agency, Government of Japan".into(),
                terms_url: EGOV_TERMS.into(),
                confidence: Confidence::Official,
                locator: "/".into(),
            },
        })
    }

    pub fn revision(&self, index: usize) -> Option<RevisionExpectation<'_>> {
        self.response.revisions.get(index).map(|info| {
            let mut source = self.source.clone();
            source.locator = format!("/law_info, /revisions/{index}");
            RevisionExpectation {
                law: &self.response.law_info,
                info,
                source,
            }
        })
    }
}
#[derive(Debug, Deserialize)]
struct DataResponse {
    law_info: LawInfo,
    revision_info: RevisionInfo,
    law_full_text: String,
}
#[derive(Debug, Deserialize)]
#[cfg(not(target_arch = "wasm32"))]
struct LawEntry {
    law_info: LawInfo,
}
#[derive(Debug, Deserialize)]
#[cfg(not(target_arch = "wasm32"))]
struct LawsResponse {
    total_count: usize,
    next_offset: Option<usize>,
    laws: Vec<LawEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Catalog {
    pub schema_version: u32,
    pub collected_at: DateTime<Utc>,
    pub source_url: String,
    pub reported_count: usize,
    pub law_ids: Vec<String>,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LawInventory {
    pub law_id: String,
    pub available_revisions: usize,
    pub revision_ids: Vec<String>,
    pub collected_at: DateTime<Utc>,
    pub source_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_revision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_snapshot_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("Law-data metadata differs from its history response at {path}")]
struct MetadataDisagreement {
    path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(not(target_arch = "wasm32"))]
struct CacheEntry {
    digest: String,
    collected_at: DateTime<Utc>,
    #[serde(default)]
    acquired_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct FetchedSource {
    pub bytes: Vec<u8>,
    pub acquired_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(not(target_arch = "wasm32"))]
struct RevisionReceipt {
    metadata_sha256: String,
    version: String,
    #[serde(default)]
    extractor_version: u32,
}

#[cfg(not(target_arch = "wasm32"))]
enum CollectedRevision {
    Reused(String),
    Saved(String),
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Fetcher {
    client: Client,
    minimum_delay: Duration,
    last_request: Option<std::time::Instant>,
    remaining: Option<usize>,
    clock: Box<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Fetcher {
    pub fn new(minimum_delay: Duration, maximum_requests: Option<usize>) -> anyhow::Result<Self> {
        Self::with_clock(minimum_delay, maximum_requests, Utc::now)
    }

    pub fn with_clock(
        minimum_delay: Duration,
        maximum_requests: Option<usize>,
        clock: impl Fn() -> DateTime<Utc> + Send + Sync + 'static,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent("law-history/0.1 (public source preservation)")
                .timeout(REQUEST_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            minimum_delay,
            last_request: None,
            remaining: maximum_requests,
            clock: Box::new(clock),
        })
    }

    pub fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }

    pub fn fetch(&mut self, store: &Store, url: &str, refresh: bool) -> anyhow::Result<Vec<u8>> {
        Ok(self.fetch_source(store, url, refresh)?.bytes)
    }

    pub fn fetch_snapshot(
        &mut self,
        store: &Store,
        url: &str,
        refresh: bool,
    ) -> anyhow::Result<FetchedSource> {
        let source = self.fetch_source(store, url, refresh)?;
        if source.acquired_at.is_none() && self.remaining != Some(0) {
            self.fetch_source(store, url, true)
        } else {
            Ok(source)
        }
    }

    pub fn fetch_revision(
        &mut self,
        store: &Store,
        url: &str,
        refresh: bool,
        expected: RevisionExpectation<'_>,
    ) -> anyhow::Result<Revision> {
        let data = self.fetch(store, url, refresh)?;
        match revision_from_expected_response(store, &data, url, &expected) {
            Err(error)
                if error.is::<MetadataDisagreement>() && !refresh && self.remaining != Some(0) =>
            {
                let revalidated = self.fetch(store, url, true)?;
                revision_from_expected_response(store, &revalidated, url, &expected)
            }
            result => result,
        }
    }

    fn fetch_source(
        &mut self,
        store: &Store,
        url: &str,
        refresh: bool,
    ) -> anyhow::Result<FetchedSource> {
        let now = self.now();
        let key = format!("cache:{}", sha256(url.as_bytes()));
        if !refresh
            && let Some(cached) = store.latest_fact::<CacheEntry>(&key, None)?
            && cached.collected_at <= now
            && cached.collected_at.date_naive() == now.date_naive()
        {
            if cached
                .acquired_at
                .is_some_and(|time| time > cached.collected_at)
            {
                bail!("Cache acquisition time follows its local observation");
            }
            return Ok(FetchedSource {
                bytes: store.source(&cached.digest)?,
                acquired_at: cached.acquired_at,
            });
        }
        if let Some(remaining) = &mut self.remaining {
            if *remaining == 0 {
                bail!("Request bound reached; preserved checkpoints can be resumed");
            }
            *remaining -= 1;
        }
        if let Some(previous) = self.last_request
            && let Some(delay) = self.minimum_delay.checked_sub(previous.elapsed())
        {
            std::thread::sleep(delay);
        }
        let result = (|| {
            let response = self.client.get(url).send()?.error_for_status()?;
            let mut bytes = Vec::new();
            response
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 64 * 1024 * 1024 {
                bail!("Response exceeds the 64 MiB input limit");
            }
            Ok::<_, anyhow::Error>(bytes)
        })();
        self.last_request = Some(std::time::Instant::now());
        let bytes = result.with_context(|| format!("Source request failed: {url}"))?;
        let digest = store.preserve(&bytes)?;
        let collected_at = self.now();
        store.fact(
            &key,
            &CacheEntry {
                digest,
                collected_at,
                acquired_at: Some(collected_at),
            },
            collected_at,
        )?;
        Ok(FetchedSource {
            bytes,
            acquired_at: Some(collected_at),
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn catalog(store: &Store, fetcher: &mut Fetcher, refresh: bool) -> anyhow::Result<Catalog> {
    let mut offset = 0;
    let mut ids = BTreeSet::new();
    let mut seen_offsets = BTreeSet::new();
    let mut reported = None;
    loop {
        if !seen_offsets.insert(offset) {
            bail!("e-Gov returned a cyclic catalog cursor");
        }
        let url = format!("{EGOV_API}/laws?limit=1000&offset={offset}&order=%2Blaw_info.law_id");
        let bytes = fetcher.fetch(store, &url, refresh)?;
        let page: LawsResponse =
            serde_json::from_slice(&bytes).context("Invalid e-Gov catalog response")?;
        if reported.is_some_and(|count| count != page.total_count) {
            bail!("e-Gov catalog changed during pagination; restart the catalog scan");
        }
        reported = Some(page.total_count);
        for law in page.laws {
            validate_id(&law.law_info.law_id).map_err(anyhow::Error::msg)?;
            ids.insert(law.law_info.law_id);
        }
        let now = fetcher.now();
        let partial = Catalog {
            schema_version: SCHEMA_VERSION,
            collected_at: now,
            source_url: format!("{EGOV_API}/laws"),
            reported_count: page.total_count,
            law_ids: ids.iter().cloned().collect(),
            complete: false,
        };
        store.fact(&format!("catalog-page:{offset}"), &partial, now)?;
        match page.next_offset {
            Some(next) if next > offset && ids.len() < page.total_count => offset = next,
            Some(next) if next <= offset => bail!("e-Gov catalog cursor did not advance"),
            _ => break,
        }
    }
    let now = fetcher.now();
    let result = Catalog {
        schema_version: SCHEMA_VERSION,
        collected_at: now,
        source_url: format!("{EGOV_API}/laws"),
        reported_count: reported.unwrap_or_default(),
        complete: ids.len() == reported.unwrap_or_default(),
        law_ids: ids.into_iter().collect(),
    };
    store.fact("catalog", &result, now)?;
    if !result.complete {
        bail!(
            "Catalog contains {} unique laws but reports {}",
            result.law_ids.len(),
            result.reported_count
        );
    }
    Ok(result)
}

fn legal_date(raw: Option<&str>, original: &str, basis: &str, scheduled: bool) -> LegalDate {
    match raw.and_then(|value| value.parse::<Day>().ok()) {
        Some(date) if !scheduled && date >= "1873-01-01".parse().expect("constant date") => {
            LegalDate::Known {
                date,
                original: original.into(),
                basis: basis.into(),
            }
        }
        _ => LegalDate::Uncertain {
            original: original.into(),
            reason: if scheduled {
                "The source supplies a scheduled date, not a confirmed enforcement date"
            } else {
                "A calendar conversion or a complete date is not established"
            }
            .into(),
        },
    }
}

pub fn revision_from_response(
    store: &impl crate::store_types::Repository,
    bytes: &[u8],
    url: &str,
) -> anyhow::Result<Revision> {
    let data: DataResponse =
        serde_json::from_slice(bytes).context("Invalid e-Gov law-data envelope")?;
    revision_from_data(store, data, bytes, url)
}

pub(crate) fn revision_from_expected_response(
    store: &impl crate::store_types::Repository,
    bytes: &[u8],
    url: &str,
    expected: &RevisionExpectation<'_>,
) -> anyhow::Result<Revision> {
    let mut data: DataResponse =
        serde_json::from_slice(bytes).context("Invalid e-Gov law-data envelope")?;
    if data.law_info.law_id != expected.law.law_id
        || data.revision_info.metadata.law_revision_id != expected.info.metadata.law_revision_id
    {
        bail!("Law-data identity differs from the requested revision");
    }
    data.law_info = merge_metadata(&data.law_info, expected.law, "/law_info")?;
    data.revision_info.metadata = data
        .revision_info
        .metadata
        .merged(&expected.info.metadata)?;
    store.source(&expected.source.raw_sha256)?;
    let mut revision = revision_from_data(store, data, bytes, url)?;
    revision.metadata_source = Some(expected.source.clone());
    revision.validate().map_err(anyhow::Error::msg)?;
    Ok(revision)
}

fn revision_from_data(
    store: &impl crate::store_types::Repository,
    data: DataResponse,
    bytes: &[u8],
    url: &str,
) -> anyhow::Result<Revision> {
    let xml_bytes = base64::engine::general_purpose::STANDARD
        .decode(&data.law_full_text)
        .context("Invalid base64 law XML")?;
    store.preserve(&xml_bytes)?;
    let xml_text = std::str::from_utf8(&xml_bytes).context("Law XML must be UTF-8")?;
    let tree = parse_xml(xml_text).map_err(anyhow::Error::msg)?;
    let info = data.revision_info.metadata;
    let original_date = format!(
        "{}; Era={}; Year={}; Month={}; Day={}",
        data.law_info.law_num,
        tree.attributes.get("Era").map(String::as_str).unwrap_or(""),
        tree.attributes
            .get("Year")
            .map(String::as_str)
            .unwrap_or(""),
        tree.attributes
            .get("PromulgateMonth")
            .map(String::as_str)
            .unwrap_or(""),
        tree.attributes
            .get("PromulgateDay")
            .map(String::as_str)
            .unwrap_or("")
    );
    let scheduled = info.amendment_scheduled_enforcement_date.is_some();
    let effective_original = info
        .amendment_enforcement_comment
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or(info.amendment_enforcement_date.as_deref())
        .or(info.amendment_scheduled_enforcement_date.as_deref())
        .unwrap_or("Date absent in source");
    let original_enactment = info.amendment_type == "1" && info.mission == "New";
    let termination_claimed =
        !matches!(info.repeal_status, SourceRepealStatus::None) || info.amendment_type == "8";
    let repeal_date = legal_date(
        info.repeal_date.as_deref(),
        info.repeal_date
            .as_deref()
            .unwrap_or("Date absent in source"),
        "e-Gov revision_info.repeal_date",
        false,
    );
    let processing_status = match info.repeal_status {
        SourceRepealStatus::None | SourceRepealStatus::Repeal => None,
        SourceRepealStatus::Expire => Some(TerminationStatus::Expired),
        SourceRepealStatus::Suspend => Some(TerminationStatus::Suspended),
        SourceRepealStatus::LossOfEffectiveness => Some(TerminationStatus::LostEffectiveness),
    };
    let date_evidence = if processing_status.is_none() && repeal_date.day().is_some() {
        law_domain::TerminationDateEvidence::Known
    } else if info
        .repeal_date
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        law_domain::TerminationDateEvidence::Uncertain
    } else {
        law_domain::TerminationDateEvidence::Absent
    };
    let end = law_domain::termination_evidence(termination_claimed, date_evidence);
    let termination = if let Some(status) = processing_status {
        Some(Termination {
            status,
            date: LegalDate::Uncertain {
                original: format!(
                    "{:?}; processing date={}",
                    info.repeal_status,
                    info.repeal_date.as_deref().unwrap_or("absent")
                ),
                reason: "The API processing date does not establish the legal termination date"
                    .into(),
            },
        })
    } else if end == law_domain::TerminationEvidence::UnknownEnd {
        Some(Termination {
            status: TerminationStatus::Repealed,
            date: repeal_date.clone(),
        })
    } else {
        None
    };
    let revision = Revision {
        schema_version: SCHEMA_VERSION,
        law_id: data.law_info.law_id,
        revision_id: info.law_revision_id,
        title: crate::xml::title_text(&info.law_title).map_err(anyhow::Error::msg)?,
        promulgated: legal_date(
            data.law_info.promulgation_date.as_deref(),
            &original_date,
            "e-Gov law_info.promulgation_date",
            false,
        ),
        amendment_promulgated: info.amendment_promulgate_date.as_deref().map(|date| {
            legal_date(
                Some(date),
                date,
                "e-Gov revision_info.amendment_promulgate_date",
                false,
            )
        }),
        effective: legal_date(
            info.amendment_enforcement_date.as_deref(),
            effective_original,
            "e-Gov revision_info.amendment_enforcement_date",
            scheduled,
        ),
        repeal: if end == law_domain::TerminationEvidence::KnownEnd {
            repeal_date.day()
        } else {
            None
        },
        termination,
        sequence: None,
        kind: if info.amendment_type == "8"
            || matches!(info.repeal_status, SourceRepealStatus::Repeal)
        {
            RevisionKind::Repeal
        } else if original_enactment {
            RevisionKind::Enactment
        } else {
            RevisionKind::Consolidated
        },
        amendment_law_id: info.amendment_law_id.filter(|value| !value.is_empty()),
        original_enactment,
        tree,
        source: Provenance {
            url: url.into(),
            raw_sha256: store.preserve(bytes)?,
            media_type: "application/json; base64 law XML".into(),
            attribution: "e-Gov Law Search, Digital Agency, Government of Japan".into(),
            terms_url: EGOV_TERMS.into(),
            confidence: Confidence::Official,
            locator: "/law_full_text".into(),
        },
        metadata_source: None,
    };
    revision.validate().map_err(anyhow::Error::msg)?;
    Ok(revision)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct SyncReport {
    pub laws: usize,
    pub saved: usize,
    pub reused: usize,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CacheImportReport {
    pub imported: usize,
    pub skipped: usize,
}

#[derive(Serialize)]
#[cfg(not(target_arch = "wasm32"))]
struct ArchivedObservation {
    source_digest: String,
    previously_recorded_at: DateTime<Utc>,
    reobserved_at: DateTime<Utc>,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn import_cache(
    store: &Store,
    archive: &Store,
    clock: impl Fn() -> DateTime<Utc>,
) -> anyhow::Result<CacheImportReport> {
    if store.root().canonicalize()? == archive.root().canonicalize()? {
        bail!("The cache archive and destination must be different stores");
    }
    let mut report = CacheImportReport::default();
    for key in archive.fact_keys("cache:")? {
        let Some(cached) = archive.latest_fact::<CacheEntry>(&key, None)? else {
            continue;
        };
        let started_at = clock();
        if cached.collected_at.date_naive() != started_at.date_naive()
            || cached.collected_at > started_at
            || store.latest_fact::<CacheEntry>(&key, None)?.is_some()
        {
            report.skipped += 1;
            continue;
        }
        let digest = store.preserve(&archive.source(&cached.digest)?)?;
        let reobserved_at = clock();
        store.fact(
            &format!("archive-{key}"),
            &ArchivedObservation {
                source_digest: digest.clone(),
                previously_recorded_at: cached.collected_at,
                reobserved_at,
            },
            reobserved_at,
        )?;
        store.fact(
            &key,
            &CacheEntry {
                digest,
                collected_at: reobserved_at,
                acquired_at: cached.acquired_at,
            },
            reobserved_at,
        )?;
        report.imported += 1;
    }
    Ok(report)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn sync_law(
    store: &Store,
    fetcher: &mut Fetcher,
    law_id: &str,
    refresh: bool,
) -> anyhow::Result<SyncReport> {
    validate_id(law_id).map_err(anyhow::Error::msg)?;
    let url = format!("{EGOV_API}/law_revisions/{law_id}");
    let source = fetcher.fetch_snapshot(store, &url, refresh)?;
    let snapshot = HistorySnapshot::from_response(store, &source.bytes, &url)?;
    let list = &snapshot.response;
    if list.law_info.law_id != law_id {
        bail!("The history response identifies a different law");
    }
    let now = fetcher.now();
    let mut inventory = LawInventory {
        law_id: law_id.into(),
        available_revisions: list.revisions.len(),
        revision_ids: list
            .revisions
            .iter()
            .map(|info| info.metadata.law_revision_id.clone())
            .collect(),
        collected_at: now,
        source_digest: store.preserve(&source.bytes)?,
        current_revision_id: {
            let mut current: BTreeSet<_> = list
                .revisions
                .iter()
                .filter(|info| info.metadata.current_revision_status == "CurrentEnforced")
                .map(|info| &info.metadata.law_revision_id)
                .collect();
            match current.len() {
                1 => current.pop_first().cloned(),
                _ => None,
            }
        },
        current_snapshot_at: source.acquired_at,
        current_version: None,
    };
    let mut result = SyncReport {
        laws: 1,
        ..Default::default()
    };
    for (index, info) in list.revisions.iter().enumerate() {
        let revision_id = &info.metadata.law_revision_id;
        let key = format!("receipt:{revision_id}");
        let metadata_sha256 = sha256(&serde_json::to_vec(&(&list.law_info, info))?);
        let reusable = if let Some(receipt) = store.latest_fact::<RevisionReceipt>(&key, None)?
            && receipt.extractor_version == EXTRACTOR_VERSION
            && receipt.metadata_sha256 == metadata_sha256
            && let Ok(preserved) = store.version(&receipt.version)
            && preserved.law_id == law_id
            && &preserved.revision_id == revision_id
        {
            Some(receipt.version)
        } else {
            None
        };
        let data_url = format!("{EGOV_API}/law_data/{revision_id}?law_full_text_format=xml");
        let attempt = if let Some(version) = reusable {
            Ok(CollectedRevision::Reused(version))
        } else {
            (|| {
                let revision = fetcher.fetch_revision(
                    store,
                    &data_url,
                    refresh,
                    snapshot
                        .revision(index)
                        .context("Revision disappeared from its source snapshot")?,
                )?;
                let observed_at = fetcher.now();
                let version = store.save_revision(&revision, observed_at)?;
                store.fact(
                    &key,
                    &RevisionReceipt {
                        metadata_sha256,
                        version: version.clone(),
                        extractor_version: EXTRACTOR_VERSION,
                    },
                    observed_at,
                )?;
                Ok::<_, anyhow::Error>(CollectedRevision::Saved(version))
            })()
        };
        match attempt {
            Ok(collected) => {
                let version = match collected {
                    CollectedRevision::Reused(version) => {
                        result.reused += 1;
                        version
                    }
                    CollectedRevision::Saved(version) => {
                        result.saved += 1;
                        version
                    }
                };
                if inventory.current_revision_id.as_deref() == Some(revision_id.as_str()) {
                    inventory.current_version = Some(version);
                }
            }
            Err(error) => result.diagnostics.push(Diagnostic::new(
                DiagnosticCode::MissingSource,
                format!("{error:#}"),
                &data_url,
                revision_id,
            )),
        }
    }
    store.fact(&format!("inventory:{law_id}"), &inventory, fetcher.now())?;
    store.fact(&format!("sync:{law_id}"), &result, fetcher.now())?;
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRevision {
    pub law_id: String,
    pub revision_id: String,
    pub title: String,
    pub promulgated: LegalDate,
    #[serde(default)]
    pub amendment_promulgated: Option<LegalDate>,
    pub effective: LegalDate,
    pub repeal: Option<Day>,
    #[serde(default)]
    pub termination: Option<Termination>,
    pub sequence: Option<u32>,
    pub kind: RevisionKind,
    pub original_enactment: bool,
    pub amendment_law_id: Option<String>,
    pub xml: String,
    pub source_url: String,
    pub terms_url: String,
    pub attribution: String,
    pub confidence: Confidence,
    #[serde(default)]
    pub metadata_source: Option<Provenance>,
    pub observed_at: DateTime<Utc>,
}

pub fn import_bundle(
    store: &impl crate::store_types::Repository,
    bytes: &[u8],
) -> anyhow::Result<SyncReport> {
    let bundle: Vec<ImportRevision> = serde_json::from_slice(bytes)?;
    let mut report = SyncReport::default();
    for item in bundle {
        let revision = Revision {
            schema_version: SCHEMA_VERSION,
            law_id: item.law_id,
            revision_id: item.revision_id,
            title: item.title,
            promulgated: item.promulgated,
            amendment_promulgated: item.amendment_promulgated,
            effective: item.effective,
            repeal: item.repeal,
            termination: item.termination,
            sequence: item.sequence,
            kind: item.kind,
            original_enactment: item.original_enactment,
            amendment_law_id: item.amendment_law_id,
            tree: parse_xml(&item.xml).map_err(anyhow::Error::msg)?,
            source: Provenance {
                url: item.source_url,
                raw_sha256: store.preserve(item.xml.as_bytes())?,
                media_type: "application/xml".into(),
                attribution: item.attribution,
                terms_url: item.terms_url,
                confidence: item.confidence,
                locator: "/Law".into(),
            },
            metadata_source: item.metadata_source,
        };
        store.save_revision(&revision, item.observed_at)?;
        report.saved += 1;
    }
    report.laws = store.law_ids()?.len();
    Ok(report)
}

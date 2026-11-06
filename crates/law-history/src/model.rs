use chrono::{DateTime, Utc};
use law_domain::CivilDay;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, str::FromStr};
use utoipa::ToSchema;

pub const SCHEMA_VERSION: u32 = 1;
const JAPAN_OFFSET_HOURS: i64 = 9;
const DAY_TEXT_LENGTH: usize = 10;
const YEAR_END: usize = 4;
const MONTH_START: usize = 5;
const MONTH_END: usize = 7;
const DAY_START: usize = 8;
const SHA256_HEX_LENGTH: usize = 64;
const MAXIMUM_ID_BYTES: usize = 200;

pub fn japan_day(now: DateTime<Utc>) -> Result<Day> {
    let local = now
        .checked_add_signed(chrono::Duration::hours(JAPAN_OFFSET_HOURS))
        .ok_or_else(|| {
            HistoryError::Invalid("Calendar clock is outside the supported range".into())
        })?;
    local
        .date_naive()
        .to_string()
        .parse()
        .map_err(HistoryError::Invalid)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(try_from = "String", into = "String")]
#[schema(value_type = String, example = "1969-12-31")]
pub struct Day(CivilDay);

impl Day {
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, String> {
        CivilDay::new(year, month, day)
            .map(Self)
            .map_err(|error| format!("Invalid calendar date: {error:?}"))
    }

    pub fn civil(self) -> CivilDay {
        self.0
    }
}

impl FromStr for Day {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        if bytes.len() != DAY_TEXT_LENGTH
            || bytes[YEAR_END] != b'-'
            || bytes[MONTH_END] != b'-'
            || bytes.iter().enumerate().any(|(index, byte)| {
                index != YEAR_END && index != MONTH_END && !byte.is_ascii_digit()
            })
        {
            return Err("Expected a calendar date in YYYY-MM-DD form".into());
        }
        Self::new(
            value[..YEAR_END].parse().map_err(|_| "Invalid year")?,
            value[MONTH_START..MONTH_END]
                .parse()
                .map_err(|_| "Invalid month")?,
            value[DAY_START..].parse().map_err(|_| "Invalid day")?,
        )
    }
}

impl TryFrom<String> for Day {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        value.parse()
    }
}
impl From<Day> for String {
    fn from(value: Day) -> Self {
        value.to_string()
    }
}
impl fmt::Display for Day {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = self.0.parts();
        write!(formatter, "{year:04}-{month:02}-{day:02}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LegalDate {
    Known {
        date: Day,
        original: String,
        basis: String,
    },
    Uncertain {
        original: String,
        reason: String,
    },
}

impl LegalDate {
    pub fn known(date: Day, basis: impl Into<String>) -> Self {
        Self::Known {
            date,
            original: date.to_string(),
            basis: basis.into(),
        }
    }
    pub fn day(&self) -> Option<Day> {
        match self {
            Self::Known { date, .. } => Some(*date),
            Self::Uncertain { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Element {
    pub tag: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    #[serde(default)]
    #[schema(no_recursion)]
    pub content: Vec<Content>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Content {
    Text(String),
    Element(Element),
}

impl Element {
    pub fn text(&self) -> String {
        self.content
            .iter()
            .map(|part| match part {
                Content::Text(text) => text.clone(),
                Content::Element(child) => child.text(),
            })
            .collect()
    }
    pub fn children(&self) -> impl Iterator<Item = &Element> {
        self.content.iter().filter_map(|part| {
            if let Content::Element(child) = part {
                Some(child)
            } else {
                None
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Official,
    Reconstructed,
    Inferred,
    Synthetic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub url: String,
    pub raw_sha256: String,
    pub media_type: String,
    pub attribution: String,
    pub terms_url: String,
    pub confidence: Confidence,
    pub locator: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevisionKind {
    Enactment,
    Amendment,
    Correction,
    Repeal,
    Consolidated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminationStatus {
    Repealed,
    Expired,
    Suspended,
    LostEffectiveness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Termination {
    pub status: TerminationStatus,
    pub date: LegalDate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub schema_version: u32,
    pub law_id: String,
    pub revision_id: String,
    pub title: String,
    pub promulgated: LegalDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amendment_promulgated: Option<LegalDate>,
    pub effective: LegalDate,
    pub repeal: Option<Day>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub termination: Option<Termination>,
    pub sequence: Option<u32>,
    pub kind: RevisionKind,
    pub amendment_law_id: Option<String>,
    pub original_enactment: bool,
    pub tree: Element,
    pub source: Provenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_source: Option<Provenance>,
}

impl Revision {
    pub fn sources(&self) -> impl Iterator<Item = &Provenance> {
        std::iter::once(&self.source).chain(self.metadata_source.iter())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err("Unsupported revision schema version".into());
        }
        validate_id(&self.law_id)?;
        validate_id(&self.revision_id)?;
        if self.title.trim().is_empty() {
            return Err("A law title is required".into());
        }
        for source in self.sources() {
            if source.raw_sha256.len() != SHA256_HEX_LENGTH
                || !source
                    .raw_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err("Expected a lowercase SHA-256 digest".into());
            }
        }
        if self.tree.tag != "Law" {
            return Err("Expected a Law root element".into());
        }
        crate::xml::validate_tree(&self.tree)?;
        if self
            .repeal
            .zip(self.effective.day())
            .is_some_and(|(end, start)| end < start)
        {
            return Err("Repeal precedes enforcement".into());
        }
        Ok(())
    }
}

pub fn validate_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAXIMUM_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        Err(
            "Identifiers must contain 1 to 200 ASCII letters, digits, hyphens, or underscores"
                .into(),
        )
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub schema_version: u32,
    pub version: String,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StoredRevision {
    pub version: String,
    pub observed_at: DateTime<Utc>,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    UnknownDate,
    AmbiguousOrder,
    MissingSource,
    UnsupportedSyntax,
    WrongBase,
    OriginalMismatch,
    Conflict,
    OfficialMismatch,
    UnresolvedReference,
    CoverageGap,
    InvalidInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub source: String,
    pub locator: String,
}

impl Diagnostic {
    pub fn new(
        code: DiagnosticCode,
        message: impl Into<String>,
        source: impl Into<String>,
        locator: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            source: source.into(),
            locator: locator.into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Coverage {
    pub law_id: String,
    pub available_revisions: Option<usize>,
    pub stored_revisions: usize,
    pub earliest_known: Option<Day>,
    pub enactment_known: bool,
    pub all_available_stored: bool,
    pub last_collected: Option<DateTime<Utc>>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Selection {
    pub selected: StoredRevision,
    pub coverage: Coverage,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub at: Option<Day>,
    pub known_at: Option<DateTime<Utc>>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchHit {
    pub law_id: String,
    pub title: String,
    pub version: String,
    pub effective: LegalDate,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LocatedElement {
    pub path: String,
    pub element: Element,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
    Moved,
    Renumbered,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Change {
    pub kind: ChangeKind,
    pub before: Option<LocatedElement>,
    pub after: Option<LocatedElement>,
    pub evidence: Option<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Diff {
    pub from: String,
    pub to: String,
    pub changes: Vec<Change>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    NotFound(String),
    #[error("Selection is ambiguous: {0:?}")]
    Ambiguous(Vec<Diagnostic>),
    #[error(transparent)]
    #[cfg(not(target_arch = "wasm32"))]
    Storage(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = HistoryError> = std::result::Result<T, E>;

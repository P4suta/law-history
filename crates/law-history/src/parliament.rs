#[cfg(not(target_arch = "wasm32"))]
use crate::ingest::Fetcher;
use crate::{Store, model::*};
use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[cfg(not(target_arch = "wasm32"))]
const SPEECH_API: &str = "https://kokkai.ndl.go.jp/api/speech";
#[cfg(not(target_arch = "wasm32"))]
const SPEECH_TERMS: &str = "https://kokkai.ndl.go.jp/api.html";
#[cfg(not(target_arch = "wasm32"))]
const SPEECH_PAGE_SIZE: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum House {
    Representatives,
    Councillors,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BillKind {
    Cabinet,
    Representatives,
    Councillors,
    Budget,
    Treaty,
    Approval,
    Consent,
    Accounts,
    Resolution,
    Rules,
    Regulations,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BillId {
    pub house: House,
    pub kind: BillKind,
    pub session: u32,
    pub number: u32,
}

impl BillId {
    pub fn key(&self) -> String {
        format!(
            "{}-{}-{}-{}",
            match self.house {
                House::Representatives => "representatives",
                House::Councillors => "councillors",
            },
            self.session,
            match self.kind {
                BillKind::Cabinet => "cabinet",
                BillKind::Representatives => "representatives",
                BillKind::Councillors => "councillors",
                BillKind::Budget => "budget",
                BillKind::Treaty => "treaty",
                BillKind::Approval => "approval",
                BillKind::Consent => "consent",
                BillKind::Accounts => "accounts",
                BillKind::Resolution => "resolution",
                BillKind::Rules => "rules",
                BillKind::Regulations => "regulations",
            },
            self.number
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SpeechLink {
    pub speech_id: String,
    pub confidence: Confidence,
    pub basis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Bill {
    pub schema_version: u32,
    pub id: BillId,
    pub title: String,
    pub enacted_law_ids: Vec<String>,
    pub submitters: Vec<String>,
    pub reason: String,
    pub speeches: Vec<SpeechLink>,
    pub source: Provenance,
}

pub fn import_bill(store: &Store, bytes: &[u8], now: DateTime<Utc>) -> anyhow::Result<Bill> {
    let bill: Bill = serde_json::from_slice(bytes)?;
    if bill.schema_version != SCHEMA_VERSION
        || bill.id.session == 0
        || bill.id.number == 0
        || bill.title.trim().is_empty()
    {
        bail!("A bill needs a supported schema, house, positive session and number, and title");
    }
    for law in &bill.enacted_law_ids {
        validate_id(law).map_err(anyhow::Error::msg)?;
    }
    store
        .source(&bill.source.raw_sha256)
        .context("Preserve the bill source before importing its linkage")?;
    store.preserve(bytes)?;
    store.fact(&format!("bill:{}", bill.id.key()), &bill, now)?;
    Ok(bill)
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Speech {
    pub speech_id: String,
    pub meeting_id: String,
    pub session: u32,
    pub house: String,
    pub meeting: String,
    pub date: Day,
    pub speaker: String,
    pub text: String,
    pub url: String,
    pub source: Provenance,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg(not(target_arch = "wasm32"))]
struct SpeechPage {
    number_of_records: usize,
    number_of_return: usize,
    start_record: usize,
    next_record_position: Option<usize>,
    speech_record: Option<Vec<SpeechRecord>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SpeechPage {
    fn validate(&self, requested_start: usize) -> anyhow::Result<()> {
        let returned = self.speech_record.as_ref().map_or(0, Vec::len);
        if self.start_record != requested_start
            || self.number_of_return != returned
            || returned > SPEECH_PAGE_SIZE
            || self.number_of_return > self.number_of_records
            || (returned == 0 && self.next_record_position.is_some_and(|next| next != 0))
        {
            bail!("The parliamentary response has inconsistent page metadata");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg(not(target_arch = "wasm32"))]
struct SpeechRecord {
    #[serde(rename = "speechID")]
    speech_id: String,
    #[serde(rename = "issueID")]
    issue_id: String,
    session: u32,
    name_of_house: String,
    name_of_meeting: String,
    date: Day,
    speaker: String,
    speech: String,
    #[serde(rename = "speechURL")]
    speech_url: String,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn sync_speeches(
    store: &Store,
    fetcher: &mut Fetcher,
    session: u32,
    query: &str,
    max_pages: usize,
) -> anyhow::Result<usize> {
    if session == 0 || query.trim().is_empty() || max_pages == 0 {
        bail!("A session, search term, and positive page bound are required");
    }
    let mut start = 1;
    let mut count = 0;
    for _ in 0..max_pages {
        let mut url = reqwest::Url::parse(SPEECH_API)?;
        url.query_pairs_mut()
            .append_pair("recordPacking", "json")
            .append_pair("maximumRecords", &SPEECH_PAGE_SIZE.to_string())
            .append_pair("sessionFrom", &session.to_string())
            .append_pair("sessionTo", &session.to_string())
            .append_pair("any", query)
            .append_pair("startRecord", &start.to_string());
        let bytes = fetcher.fetch(store, url.as_str(), false)?;
        let page: SpeechPage = serde_json::from_slice(&bytes)?;
        page.validate(start)?;
        let records = page.speech_record.unwrap_or_default();
        let raw_sha256 = store.preserve(&bytes)?;
        for record in records {
            validate_id(&record.speech_id).map_err(anyhow::Error::msg)?;
            let speech = Speech {
                speech_id: record.speech_id,
                meeting_id: record.issue_id,
                session: record.session,
                house: record.name_of_house,
                meeting: record.name_of_meeting,
                date: record.date,
                speaker: record.speaker,
                text: record.speech,
                url: record.speech_url,
                source: Provenance {
                    url: url.to_string(),
                    raw_sha256: raw_sha256.clone(),
                    media_type: "application/json".into(),
                    attribution:
                        "National Diet Library; individual speech rights remain with the speaker"
                            .into(),
                    terms_url: SPEECH_TERMS.into(),
                    confidence: Confidence::Official,
                    locator: "/speechRecord".into(),
                },
            };
            store.fact(
                &format!("speech:{}", speech.speech_id),
                &speech,
                fetcher.now(),
            )?;
            count += 1;
        }
        match page.next_record_position {
            Some(next) if next > start => start = next,
            Some(0) | None => return Ok(count),
            _ => bail!("The parliamentary API returned a non-advancing cursor"),
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_page_validation_enforces_counts_bounds_and_empty_cursors() {
        let record = serde_json::json!({"speechID":"SP1","issueID":"MT1","session":1,"nameOfHouse":"House","nameOfMeeting":"Meeting","date":"1969-12-31","speaker":"Speaker","speech":"Speech","speechURL":"https://example.invalid/speech"});
        for (rows, total, returned, next, valid) in [
            (0, 0, 0, None, true),
            (0, 0, 0, Some(0), true),
            (0, 0, 0, Some(1), false),
            (1, 2, 1, Some(2), true),
            (100, 100, 100, None, true),
            (101, 101, 101, None, false),
            (1, 0, 1, None, false),
            (1, 2, 2, None, false),
        ] {
            let page: SpeechPage = serde_json::from_value(serde_json::json!({"numberOfRecords":total,"numberOfReturn":returned,"startRecord":1,"nextRecordPosition":next,"speechRecord":vec![record.clone();rows]})).unwrap();
            assert_eq!(
                page.validate(1).is_ok(),
                valid,
                "{rows} {total} {returned} {next:?}"
            );
        }
    }

    #[test]
    fn bill_categories_prevent_same_number_collisions() {
        let cabinet = BillId {
            house: House::Representatives,
            kind: BillKind::Cabinet,
            session: 217,
            number: 1,
        };
        let member = BillId {
            kind: BillKind::Representatives,
            ..cabinet.clone()
        };
        assert_ne!(cabinet.key(), member.key());
        assert_ne!(
            cabinet.key(),
            BillId {
                house: House::Councillors,
                ..cabinet.clone()
            }
            .key()
        );
    }

    #[test]
    fn empty_speech_pages_and_error_responses_remain_distinct() {
        let empty: SpeechPage = serde_json::from_str(
            r#"{"numberOfRecords":0,"numberOfReturn":0,"startRecord":1,"speechRecord":null}"#,
        )
        .unwrap();
        assert!(empty.validate(1).is_ok());
        assert!(empty.validate(2).is_err());
        assert!(serde_json::from_str::<SpeechPage>(r#"{"message":"Invalid query"}"#).is_err());
        let inconsistent: SpeechPage = serde_json::from_str(
            r#"{"numberOfRecords":1,"numberOfReturn":1,"startRecord":1,"speechRecord":[]}"#,
        )
        .unwrap();
        assert!(inconsistent.validate(1).is_err());
    }
}

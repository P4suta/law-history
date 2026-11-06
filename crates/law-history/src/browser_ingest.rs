use crate::{
    ingest::{self, HistorySnapshot, LawInventory},
    model::*,
    store_types::Repository,
};
use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const CATALOG_LIMIT: usize = 20;

#[derive(Serialize, Deserialize)]
pub struct CatalogMatches {
    pub total_count: usize,
    pub laws: Vec<CatalogLaw>,
}

#[derive(Serialize, Deserialize)]
pub struct CatalogLaw {
    pub law_info: ingest::LawInfo,
    pub revision_info: CatalogTitle,
}

#[derive(Serialize, Deserialize)]
pub struct CatalogTitle {
    pub law_title: String,
}

pub fn catalog(bytes: &[u8]) -> anyhow::Result<CatalogMatches> {
    let mut result: CatalogMatches = serde_json::from_slice(bytes)?;
    if result.total_count < result.laws.len()
        || !law_domain::input_fits(result.laws.len(), CATALOG_LIMIT)
    {
        bail!("The official catalog has inconsistent result counts");
    }
    let mut ids = std::collections::BTreeSet::new();
    for law in &mut result.laws {
        validate_id(&law.law_info.law_id).map_err(anyhow::Error::msg)?;
        if !ids.insert(&law.law_info.law_id) {
            bail!("The official catalog repeats a law identifier");
        }
        law.revision_info.law_title =
            crate::xml::title_text(&law.revision_info.law_title).map_err(anyhow::Error::msg)?;
        if law.revision_info.law_title.trim().is_empty() {
            bail!("An official catalog entry needs a visible law title");
        }
    }
    Ok(result)
}

pub fn history(
    store: &impl Repository,
    law_id: &str,
    bytes: &[u8],
    observed_at: DateTime<Utc>,
) -> anyhow::Result<LawInventory> {
    validate_id(law_id).map_err(anyhow::Error::msg)?;
    let url = format!("{}/law_revisions/{law_id}", ingest::EGOV_API);
    let snapshot = HistorySnapshot::from_response(store, bytes, &url)?;
    if snapshot.response.law_info.law_id != law_id {
        bail!("The history response identifies a different law");
    }
    let revisions = &snapshot.response.revisions;
    let ids: std::collections::BTreeSet<_> = revisions
        .iter()
        .map(|row| row.metadata.law_revision_id.clone())
        .collect();
    if ids.len() != revisions.len() || ids.is_empty() {
        bail!("A law history must contain unique revision identifiers");
    }
    let current: Vec<_> = revisions
        .iter()
        .filter(|row| row.metadata.current_revision_status == "CurrentEnforced")
        .collect();
    let current_revision_id = if current.len() == 1 {
        Some(current[0].metadata.law_revision_id.clone())
    } else {
        None
    };
    let inventory = LawInventory {
        law_id: law_id.into(),
        available_revisions: revisions.len(),
        revision_ids: revisions
            .iter()
            .map(|row| row.metadata.law_revision_id.clone())
            .collect(),
        collected_at: observed_at,
        source_digest: store.preserve(bytes)?,
        current_revision_id,
        current_snapshot_at: Some(observed_at),
        current_version: None,
    };
    store.fact(&format!("inventory:{law_id}"), &inventory, observed_at)?;
    Ok(inventory)
}

pub fn revision(
    store: &impl Repository,
    law_id: &str,
    revision_id: &str,
    history_digest: &str,
    bytes: &[u8],
    observed_at: DateTime<Utc>,
) -> anyhow::Result<String> {
    validate_id(law_id).map_err(anyhow::Error::msg)?;
    validate_id(revision_id).map_err(anyhow::Error::msg)?;
    let mut inventory: LawInventory = store
        .latest_fact(&format!("inventory:{law_id}"), None)?
        .context("Collect the law history before its revisions")?;
    if inventory.law_id != law_id {
        bail!("The inventory fact identifies a different law");
    }
    if inventory.source_digest != history_digest {
        bail!("The law history changed during collection; restart collection");
    }
    let history_url = format!("{}/law_revisions/{law_id}", ingest::EGOV_API);
    let snapshot =
        HistorySnapshot::from_response(store, &store.source(history_digest)?, &history_url)?;
    if snapshot.response.law_info.law_id != law_id {
        bail!("The preserved history identifies a different law");
    }
    let index = snapshot
        .response
        .revisions
        .iter()
        .position(|row| row.metadata.law_revision_id == revision_id)
        .context("The revision is absent from its history")?;
    let url = format!(
        "{}/law_data/{revision_id}?law_full_text_format=xml",
        ingest::EGOV_API
    );
    let revision = ingest::revision_from_expected_response(
        store,
        bytes,
        &url,
        &snapshot
            .revision(index)
            .context("Missing revision expectation")?,
    )?;
    let version = store.save_revision(&revision, observed_at)?;
    if inventory.current_revision_id.as_deref() == Some(revision_id) {
        inventory.current_version = Some(version.clone());
    }
    inventory.collected_at = observed_at;
    store.fact(&format!("inventory:{law_id}"), &inventory, observed_at)?;
    Ok(version)
}

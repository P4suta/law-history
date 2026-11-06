use crate::{Store, amendment::Scenario, model::*, service, store::sha256};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OfficialComparison {
    pub schema_version: u32,
    pub scenario: Scenario,
    pub expected_versions: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ComparisonResult {
    pub scenario_id: String,
    pub matched_versions: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn compare(store: &Store, comparison: &OfficialComparison) -> Result<ComparisonResult> {
    if comparison.schema_version != SCHEMA_VERSION {
        return Err(HistoryError::Invalid(
            "Unsupported comparison schema".into(),
        ));
    }
    let simulation = service::simulate(store, &comparison.scenario)?;
    let mut result = ComparisonResult {
        scenario_id: simulation.scenario_id,
        matched_versions: Vec::new(),
        diagnostics: simulation.diagnostics,
    };
    if !result.diagnostics.is_empty() {
        return Ok(result);
    }
    if simulation
        .results
        .keys()
        .ne(comparison.expected_versions.keys())
    {
        return Err(HistoryError::Invalid(
            "An official comparison must cover the complete target law set".into(),
        ));
    }
    for (law, version) in &comparison.expected_versions {
        if comparison.scenario.bases.get(law) == Some(version) {
            return Err(HistoryError::Invalid(
                "An official comparison needs an independent target version after the base".into(),
            ));
        }
        let expected = store.version(version)?;
        if expected.law_id != *law || expected.source.confidence != Confidence::Official {
            return Err(HistoryError::Invalid(
                "Expected versions must be official revisions of the named law".into(),
            ));
        }
        let actual = &simulation.results[law];
        if actual.tree != expected.tree
            || actual.repeal != expected.repeal
            || actual.termination != expected.termination
        {
            result.diagnostics.push(Diagnostic::new(
                DiagnosticCode::OfficialMismatch,
                format!("Amendment output differs from official version {version}"),
                &expected.source.url,
                &expected.source.locator,
            ));
        } else {
            result.matched_versions.push(version.clone());
        }
    }
    Ok(result)
}

pub fn record_comparison(
    store: &Store,
    bytes: &[u8],
    now: DateTime<Utc>,
) -> Result<ComparisonResult> {
    let comparison: OfficialComparison = serde_json::from_slice(bytes)?;
    let result = compare(store, &comparison)?;
    store.preserve(bytes)?;
    store.fact(
        &format!("comparison-result:{}", result.scenario_id),
        &result,
        now,
    )?;
    if result.diagnostics.is_empty() {
        store.fact(
            &format!("comparison:{}", result.scenario_id),
            &comparison,
            now,
        )?;
    }
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reconstruction {
    pub schema_version: u32,
    pub scenario: Scenario,
    pub revision_ids: BTreeMap<String, String>,
    pub evidence: Provenance,
}

pub fn reconstruct(store: &Store, bytes: &[u8], now: DateTime<Utc>) -> Result<Vec<String>> {
    let reconstruction: Reconstruction = serde_json::from_slice(bytes)?;
    if reconstruction.schema_version != SCHEMA_VERSION {
        return Err(HistoryError::Invalid(
            "Unsupported reconstruction schema".into(),
        ));
    }
    store.source(&reconstruction.evidence.raw_sha256)?;
    let simulation = service::simulate(store, &reconstruction.scenario)?;
    if !simulation.diagnostics.is_empty() {
        return Err(HistoryError::Ambiguous(simulation.diagnostics));
    }
    if simulation
        .results
        .keys()
        .ne(reconstruction.revision_ids.keys())
    {
        return Err(HistoryError::Invalid(
            "Every reconstructed law needs a revision identifier".into(),
        ));
    }
    let digest = store.preserve(bytes)?;
    let mut revisions = Vec::new();
    for (law, mut revision) in simulation.results {
        revision.revision_id = reconstruction.revision_ids[&law].clone();
        revision.original_enactment = false;
        revision.kind = RevisionKind::Amendment;
        revision.metadata_source = None;
        revision.source = Provenance {
            raw_sha256: digest.clone(),
            confidence: Confidence::Reconstructed,
            locator: format!("/scenario/results/{law}"),
            ..reconstruction.evidence.clone()
        };
        revision.validate().map_err(HistoryError::Invalid)?;
        revisions.push(revision);
    }
    let versions = store.save_revisions(&revisions, now)?;
    store.fact(&format!("reconstruction:{}", sha256(bytes)), &versions, now)?;
    Ok(versions)
}

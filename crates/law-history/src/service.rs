use crate::{
    amendment::{self, Scenario, Simulation},
    ingest::{Catalog, LawInventory, SyncReport},
    model::*,
    parliament::{Bill, Speech},
    references::{self, Impact, Position, ReferenceReport},
    store::{IndexedRevision, TemporalMetadata, sha256},
    store_types::Repository as Store,
    xml,
};
use chrono::{DateTime, Utc};
use law_domain::{EffectiveState, effective_state};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(target_arch = "wasm32"))]
use std::{fs, path::Path};
use utoipa::ToSchema;

struct CanonicalHistory {
    revisions: BTreeMap<String, IndexedRevision>,
    diagnostics: Vec<Diagnostic>,
}

fn latest_observations(mut history: Vec<IndexedRevision>) -> CanonicalHistory {
    history.sort_by_key(|revision| std::cmp::Reverse(revision.observed_at));
    let mut revisions = BTreeMap::<String, IndexedRevision>::new();
    let mut conflicts = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for item in history {
        if let Some(previous) = revisions.get(&item.metadata.revision_id) {
            match law_domain::observation_choice(
                item.observed_at.cmp(&previous.observed_at),
                previous.version == item.version,
            ) {
                law_domain::ObservationChoice::Conflict => {
                    if conflicts.insert(item.metadata.revision_id.clone()) {
                        diagnostics.push(Diagnostic::new(
                            DiagnosticCode::AmbiguousOrder,
                            "Two corrections were observed at the same instant",
                            &item.metadata.source.url,
                            &item.metadata.revision_id,
                        ));
                    }
                    continue;
                }
                law_domain::ObservationChoice::Keep => continue,
                law_domain::ObservationChoice::Replace => {}
            }
        }
        revisions.insert(item.metadata.revision_id.clone(), item);
    }
    revisions.retain(|id, _| !conflicts.contains(id));
    CanonicalHistory {
        revisions,
        diagnostics,
    }
}

pub fn coverage(
    store: &impl Store,
    law_id: &str,
    known_at: Option<DateTime<Utc>>,
) -> Result<Coverage> {
    let history = store.indexed_history(law_id, known_at)?;
    let inventory: Option<LawInventory> =
        store.latest_fact(&format!("inventory:{law_id}"), known_at)?;
    let failures: Option<SyncReport> = store.latest_fact(&format!("sync:{law_id}"), known_at)?;
    let ids: BTreeSet<_> = history
        .iter()
        .map(|item| item.metadata.revision_id.clone())
        .collect();
    let latest_observed = history.iter().map(|item| item.observed_at).max();
    let canonical = latest_observations(history);
    let history: Vec<_> = canonical.revisions.values().collect();
    let mut result = Coverage {
        law_id: law_id.into(),
        available_revisions: inventory.as_ref().map(|item| item.available_revisions),
        stored_revisions: ids.len(),
        earliest_known: history
            .iter()
            .filter_map(|item| item.metadata.effective.day())
            .min(),
        enactment_known: history.iter().any(|item| item.metadata.original_enactment),
        all_available_stored: inventory
            .as_ref()
            .is_some_and(|item| item.revision_ids.iter().all(|id| ids.contains(id.as_str()))),
        last_collected: inventory
            .as_ref()
            .map(|item| item.collected_at)
            .or(latest_observed),
        diagnostics: failures.map(|item| item.diagnostics).unwrap_or_default(),
    };
    result.diagnostics.extend(canonical.diagnostics);
    if !result.enactment_known {
        result.diagnostics.push(Diagnostic::new(
            DiagnosticCode::CoverageGap,
            "The earliest stored revision is not established as the enactment text",
            "local corpus",
            law_id,
        ));
    }
    Ok(result)
}

fn load_indexed(store: &impl Store, indexed: IndexedRevision) -> Result<StoredRevision> {
    let revision = store.version(&indexed.version)?;
    if TemporalMetadata::from(&revision) != indexed.metadata {
        return Err(HistoryError::Invalid(
            "Indexed temporal metadata differs from the preserved version; rebuild the index"
                .into(),
        ));
    }
    Ok(StoredRevision {
        version: indexed.version,
        observed_at: indexed.observed_at,
        revision,
    })
}

pub fn select(store: &impl Store, law_id: &str, query: &Query, today: Day) -> Result<Selection> {
    validate_id(law_id).map_err(HistoryError::Invalid)?;
    let history = store.indexed_history(law_id, query.known_at)?;
    if let Some(version) = &query.version {
        let selected = history
            .into_iter()
            .find(|item| &item.version == version)
            .ok_or_else(|| {
                HistoryError::NotFound(
                    "The pinned version is absent at the knowledge cutoff".into(),
                )
            })?;
        let selected = load_indexed(store, selected)?;
        let diagnostics = termination_date_diagnostic(&selected.revision)
            .into_iter()
            .collect();
        return Ok(Selection {
            selected,
            coverage: coverage(store, law_id, query.known_at)?,
            diagnostics,
        });
    }
    let at = query.at.unwrap_or(today);
    let canonical = latest_observations(history);
    if !canonical.diagnostics.is_empty() {
        return Err(HistoryError::Ambiguous(canonical.diagnostics));
    }
    let revisions = canonical.revisions;
    let unknown: Vec<_> = revisions
        .values()
        .filter(|item| {
            item.metadata.effective.day().is_none()
                && item
                    .metadata
                    .amendment_promulgated
                    .as_ref()
                    .unwrap_or(&item.metadata.promulgated)
                    .day()
                    .is_none_or(|date| date <= at)
        })
        .map(|item| {
            Diagnostic::new(
                DiagnosticCode::UnknownDate,
                "Enforcement requires unresolved date evidence",
                &item.metadata.source.url,
                &item.metadata.revision_id,
            )
        })
        .collect();
    if at > today && !unknown.is_empty() {
        return Err(HistoryError::Ambiguous(unknown));
    }
    let mut candidates: Vec<_> = revisions
        .into_values()
        .filter(|item| item.metadata.effective.day().is_some_and(|date| date <= at))
        .collect();
    let latest = candidates
        .iter()
        .filter_map(|item| item.metadata.effective.day())
        .max();
    candidates.retain(|item| item.metadata.effective.day() == latest);
    if candidates.is_empty() && !unknown.is_empty() {
        return Err(HistoryError::Ambiguous(unknown));
    }
    let official_current = current_candidate(store, law_id, query.known_at, at, &candidates)?;
    let sequences: BTreeSet<_> = candidates
        .iter()
        .filter_map(|item| item.metadata.sequence)
        .collect();
    if law_domain::enforcement_group_is_ordered(candidates.len(), sequences.len()) {
        if let Some(sequence) = sequences.last().copied() {
            candidates.retain(|item| item.metadata.sequence == Some(sequence));
        }
        if let Some(current) = &official_current
            && candidates
                .iter()
                .all(|item| &item.metadata.revision_id != current)
        {
            return Err(HistoryError::Ambiguous(
                candidates
                    .iter()
                    .map(|item| {
                        Diagnostic::new(
                            DiagnosticCode::AmbiguousOrder,
                            "Enforcement sequence conflicts with the API current-state snapshot",
                            &item.metadata.source.url,
                            &item.metadata.revision_id,
                        )
                    })
                    .collect(),
            ));
        }
    } else if let Some(current) = official_current {
        candidates.retain(|item| item.metadata.revision_id == current);
    } else {
        return Err(HistoryError::Ambiguous(candidates.iter().map(|item| Diagnostic::new(DiagnosticCode::AmbiguousOrder, "Same-day official revisions need evidenced ordering or an explicit version", &item.metadata.source.url, &item.metadata.revision_id)).collect()));
    }
    let selected = candidates.pop().ok_or_else(|| {
        HistoryError::NotFound(format!(
            "No confirmed revision is available for {law_id} at {at}"
        ))
    })?;
    let selected = load_indexed(store, selected)?;
    if let Some(diagnostic) = termination_date_diagnostic(&selected.revision) {
        return Err(HistoryError::Ambiguous(vec![diagnostic]));
    }
    if let Some(termination) = &selected.revision.termination {
        match termination.date.day() {
            Some(date) if date <= at => {
                return Err(HistoryError::NotFound(format!(
                    "{law_id} has a terminated status at {at}"
                )));
            }
            None => {
                return Err(HistoryError::Ambiguous(vec![Diagnostic::new(
                    DiagnosticCode::UnknownDate,
                    "The source termination status has no established legal date; select an explicit version for inspection",
                    &selected.revision.source.url,
                    &selected.revision.source.locator,
                )]));
            }
            Some(_) => {}
        }
    }
    if effective_state(
        selected.revision.effective.day().map(Day::civil),
        selected.revision.repeal.map(Day::civil),
        at.civil(),
    ) != EffectiveState::Effective
    {
        return Err(HistoryError::NotFound(format!(
            "{law_id} is repealed at {at}"
        )));
    }
    Ok(Selection {
        selected,
        coverage: coverage(store, law_id, query.known_at)?,
        diagnostics: unknown,
    })
}

fn termination_date_diagnostic(revision: &Revision) -> Option<Diagnostic> {
    let established = revision.repeal.is_some()
        || revision
            .termination
            .as_ref()
            .and_then(|end| end.date.day())
            .is_some();
    let date = if established {
        law_domain::TerminationDateEvidence::Known
    } else if revision.termination.is_some() {
        law_domain::TerminationDateEvidence::Uncertain
    } else {
        law_domain::TerminationDateEvidence::Absent
    };
    (law_domain::termination_evidence(revision.kind == RevisionKind::Repeal, date)
        == law_domain::TerminationEvidence::UnknownEnd)
        .then(|| Diagnostic::new(
            DiagnosticCode::UnknownDate,
            "The termination evidence has no established legal date; select an explicit version for inspection",
            &revision.source.url,
            &revision.source.locator,
        ))
}

fn current_candidate(
    store: &impl Store,
    law: &str,
    known_at: Option<DateTime<Utc>>,
    at: Day,
    candidates: &[IndexedRevision],
) -> Result<Option<String>> {
    let Some(inventory) =
        store.latest_fact::<LawInventory>(&format!("inventory:{law}"), known_at)?
    else {
        return Ok(None);
    };
    if inventory.law_id != law {
        return Err(HistoryError::Invalid(
            "Inventory law identity differs from the requested law".into(),
        ));
    }
    let Some(current) = inventory.current_revision_id else {
        return Ok(None);
    };
    let Some(acquired_at) = inventory.current_snapshot_at else {
        return Ok(None);
    };
    let Some(version) = inventory.current_version else {
        return Ok(None);
    };
    if acquired_at > inventory.collected_at {
        return Err(HistoryError::Invalid(
            "Inventory acquisition time follows its local observation".into(),
        ));
    }
    let matches = candidates
        .iter()
        .any(|item| item.metadata.revision_id == current && item.version == version);
    Ok(law_domain::current_snapshot_is_evidence(
        japan_day(acquired_at)?.civil(),
        at.civil(),
        matches,
    )
    .then_some(current))
}

pub fn search(store: &impl Store, text: &str, query: &Query, today: Day) -> Result<SearchResponse> {
    let mut hits = Vec::new();
    let mut diagnostics = Vec::new();
    for law_id in store.law_ids()? {
        match select(store, &law_id, query, today) {
            Ok(selection) => {
                let revision = &selection.selected.revision;
                if law_id.contains(text)
                    || revision.title.contains(text)
                    || revision.tree.text().contains(text)
                    || crate::xml::base_text(&revision.tree).contains(text)
                {
                    hits.push(SearchHit {
                        law_id: law_id.clone(),
                        title: revision.title.clone(),
                        version: selection.selected.version,
                        effective: revision.effective.clone(),
                        coverage: selection.coverage,
                    });
                }
            }
            Err(HistoryError::NotFound(_)) => {}
            Err(HistoryError::Ambiguous(items)) => diagnostics.extend(items),
            Err(error) => return Err(error),
        }
    }
    Ok(SearchResponse { hits, diagnostics })
}

pub fn diff(store: &impl Store, from: &str, to: &str) -> Result<Diff> {
    let first = store.version(from)?;
    let second = store.version(to)?;
    if first.law_id != second.law_id {
        return Err(HistoryError::Invalid(
            "A structural diff requires two versions of the same law".into(),
        ));
    }
    Ok(amendment::structural_diff(
        &first.tree,
        &second.tree,
        from,
        to,
    ))
}

struct ReferenceContext {
    revisions: BTreeMap<String, (String, Revision)>,
    diagnostics: Vec<Diagnostic>,
}

impl ReferenceContext {
    fn versions(&self) -> BTreeMap<String, String> {
        self.revisions
            .iter()
            .map(|(law, (version, _))| (law.clone(), version.clone()))
            .collect()
    }
}

fn corpus(store: &impl Store, query: &Query, today: Day) -> Result<ReferenceContext> {
    let mut selected = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for law in store.law_ids()? {
        let temporal = Query {
            version: None,
            ..query.clone()
        };
        match select(store, &law, &temporal, today) {
            Ok(item) => {
                diagnostics.extend(item.diagnostics);
                selected.insert(law, (item.selected.version, item.selected.revision));
            }
            Err(HistoryError::NotFound(_)) => {}
            Err(HistoryError::Ambiguous(items)) => diagnostics.extend(items),
            Err(error) => return Err(error),
        }
    }
    Ok(ReferenceContext {
        revisions: selected,
        diagnostics,
    })
}

pub fn refs(store: &impl Store, law: &str, query: &Query, today: Day) -> Result<ReferenceReport> {
    let mut corpus = corpus(store, query, today)?;
    if query.version.is_some() {
        let selected = select(store, law, query, today)?;
        corpus.revisions.insert(
            law.into(),
            (selected.selected.version, selected.selected.revision),
        );
    }
    let references = references::analyze(&corpus.revisions)
        .into_iter()
        .filter(|edge| {
            edge.from.law_id == law || edge.to.as_ref().is_some_and(|target| target.law_id == law)
        })
        .collect();
    Ok(ReferenceReport {
        references,
        context_versions: corpus.versions(),
        diagnostics: corpus.diagnostics,
    })
}

pub fn impact(
    store: &impl Store,
    from: &str,
    to: &str,
    query: &Query,
    today: Day,
) -> Result<Impact> {
    let revision = store.version(from)?;
    let diff = diff(store, from, to)?;
    let mut corpus = corpus(store, query, today)?;
    corpus
        .revisions
        .insert(revision.law_id.clone(), (from.into(), revision.clone()));
    let direct = diff
        .changes
        .iter()
        .filter_map(|change| {
            change
                .before
                .as_ref()
                .or(change.after.as_ref())
                .map(|node| Position {
                    law_id: revision.law_id.clone(),
                    version: if change.before.is_some() { from } else { to }.into(),
                    path: node.path.clone(),
                })
        })
        .collect();
    let mut result = references::impact(&references::analyze(&corpus.revisions), direct);
    result.context_versions = corpus.versions();
    let titles = corpus
        .revisions
        .iter()
        .map(|(law, (_, revision))| (law.clone(), revision.title.clone()))
        .collect();
    corpus
        .revisions
        .insert(revision.law_id, (to.into(), store.version(to)?));
    result.broken = references::analyze_with_titles(&corpus.revisions, &titles)
        .into_iter()
        .filter(|edge| edge.to.is_none())
        .collect();
    result.diagnostics = corpus.diagnostics;
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BlameEntry {
    pub path: String,
    pub version: String,
    pub previous_version: Option<String>,
    pub source: Provenance,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn blame(
    store: &impl Store,
    law: &str,
    path: &str,
    query: &Query,
    today: Day,
) -> Result<BlameEntry> {
    let selection = select(store, law, query, today)?;
    let selected = selection.selected;
    let target = xml::flatten(&selected.revision.tree)
        .remove(path)
        .ok_or_else(|| HistoryError::NotFound(format!("No element at {path}")))?;
    let selected_day = selected.revision.effective.day().ok_or_else(|| {
        HistoryError::Ambiguous(vec![Diagnostic::new(
            DiagnosticCode::UnknownDate,
            "A chronological introduction requires an established enforcement date",
            &selected.revision.source.url,
            path,
        )])
    })?;
    let mut history = store.indexed_history(law, query.known_at)?;
    if query.version.is_some() {
        history.retain(|item| {
            item.metadata.revision_id != selected.revision.revision_id
                || item.version == selected.version
        });
    }
    let canonical = latest_observations(history);
    if !canonical.diagnostics.is_empty() {
        return Err(HistoryError::Ambiguous(canonical.diagnostics));
    }
    let mut groups = BTreeMap::<Day, Vec<IndexedRevision>>::new();
    for item in canonical.revisions.into_values() {
        if let Some(date) = item.metadata.effective.day()
            && date <= selected_day
        {
            groups.entry(date).or_default().push(item);
        }
    }
    let mut history = Vec::new();
    for mut group in groups.into_values() {
        let sequences: BTreeSet<_> = group
            .iter()
            .filter_map(|item| item.metadata.sequence)
            .collect();
        if !law_domain::enforcement_group_is_ordered(group.len(), sequences.len()) {
            return Err(HistoryError::Ambiguous(group.iter().map(|item| Diagnostic::new(DiagnosticCode::AmbiguousOrder, "Chronological attribution requires evidenced same-day revision ordering", &item.metadata.source.url, &item.metadata.revision_id)).collect()));
        }
        group.sort_by_key(|item| item.metadata.sequence);
        history.extend(group);
    }
    let position = history
        .iter()
        .position(|item| item.version == selected.version)
        .ok_or_else(|| {
            HistoryError::NotFound(
                "The selected revision has no established timeline position".into(),
            )
        })?;
    history.truncate(position);
    history.reverse();
    let mut origin = selected;
    let mut previous = None;
    for indexed in history {
        let item = load_indexed(store, indexed)?;
        if xml::flatten(&item.revision.tree).get(path) == Some(&target) {
            origin = item;
        } else {
            previous = Some(item.version);
            break;
        }
    }
    let mut diagnostics = selection.diagnostics;
    if previous.is_none() && !origin.revision.original_enactment {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::CoverageGap,
            "The text exists in the earliest known revision; its original introduction is not established",
            &origin.revision.source.url,
            path,
        ));
    }
    Ok(BlameEntry {
        path: path.into(),
        version: origin.version,
        previous_version: previous,
        source: origin.revision.source,
        diagnostics,
    })
}

pub fn simulate(store: &impl Store, scenario: &Scenario) -> Result<Simulation> {
    let mut bases = BTreeMap::new();
    for (law, version) in &scenario.bases {
        if let Some(cutoff) = scenario.known_at
            && !store
                .history(law, Some(cutoff))?
                .iter()
                .any(|item| &item.version == version)
        {
            return Err(HistoryError::NotFound(
                "A scenario base was not known at its knowledge cutoff".into(),
            ));
        }
        bases.insert(law.clone(), store.version(version)?);
    }
    let mut result = amendment::simulate(scenario, &bases);
    if result.diagnostics.is_empty() {
        let mut context = scenario_context(store, scenario)?;
        for (law, revision) in &bases {
            context
                .revisions
                .insert(law.clone(), (scenario.bases[law].clone(), revision.clone()));
        }
        let before = context.revisions;
        let mut after = before.clone();
        for (law, revision) in &result.results {
            if revision.repeal.is_some_and(|date| date <= scenario.at) {
                after.remove(law);
            } else {
                after.insert(law.clone(), (result.scenario_id.clone(), revision.clone()));
            }
        }
        let before_edges = references::analyze(&before);
        let titles = before
            .iter()
            .map(|(law, (_, revision))| (law.clone(), revision.title.clone()))
            .collect();
        let after_edges = references::analyze_with_titles(&after, &titles);
        let mut direct = Vec::new();
        for difference in &result.diffs {
            if let Some((law, _)) = scenario
                .bases
                .iter()
                .find(|(_, version)| *version == &difference.from)
            {
                for change in &difference.changes {
                    if let Some(node) = change.before.as_ref().or(change.after.as_ref()) {
                        direct.push(Position {
                            law_id: law.clone(),
                            version: if change.before.is_some() {
                                difference.from.clone()
                            } else {
                                result.scenario_id.clone()
                            },
                            path: node.path.clone(),
                        });
                    }
                }
            }
        }
        for (law, revision) in &result.results {
            if bases[law].repeal != revision.repeal {
                direct.push(Position {
                    law_id: law.clone(),
                    version: scenario.bases[law].clone(),
                    path: "/Law".into(),
                });
            }
        }
        result.impact = references::impact(&before_edges, direct);
        result.impact.context_versions = before
            .iter()
            .map(|(law, (version, _))| (law.clone(), version.clone()))
            .collect();
        result.impact.diagnostics = context.diagnostics;
        result.impact.broken = after_edges
            .into_iter()
            .filter(|edge| {
                edge.to.is_none()
                    && !before_edges.iter().any(|previous| {
                        previous.to.is_none()
                            && previous.from.law_id == edge.from.law_id
                            && previous.from.path == edge.from.path
                            && previous.text == edge.text
                    })
            })
            .collect();
    }
    Ok(result)
}

fn scenario_context(store: &impl Store, scenario: &Scenario) -> Result<ReferenceContext> {
    match &scenario.reference_context {
        Some(pinned) => {
            let mut revisions = BTreeMap::new();
            for (law, version) in &pinned.versions {
                if scenario.bases.contains_key(law) {
                    return Err(HistoryError::Invalid(
                        "Pinned reference laws must be separate from scenario bases".into(),
                    ));
                }
                let selected = select(
                    store,
                    law,
                    &Query {
                        at: Some(scenario.at),
                        known_at: scenario.known_at,
                        version: Some(version.clone()),
                    },
                    scenario.at,
                )?;
                revisions.insert(law.clone(), (version.clone(), selected.selected.revision));
            }
            Ok(ReferenceContext {
                revisions,
                diagnostics: pinned.diagnostics.clone(),
            })
        }
        None => corpus(
            store,
            &Query {
                at: Some(scenario.at),
                known_at: scenario.known_at,
                version: None,
            },
            scenario.at,
        ),
    }
}

pub fn save_scenario(
    store: &impl Store,
    scenario: &Scenario,
    now: DateTime<Utc>,
) -> Result<String> {
    let mut fixed = scenario.clone();
    fixed.known_at.get_or_insert(now);
    if fixed.reference_context.is_none() {
        let mut context = scenario_context(store, &fixed)?;
        context
            .revisions
            .retain(|law, _| !fixed.bases.contains_key(law));
        fixed.reference_context = Some(crate::amendment::PinnedReferenceContext {
            versions: context.versions(),
            diagnostics: context.diagnostics,
        });
    }
    let simulation = simulate(store, &fixed)?;
    if !simulation.diagnostics.is_empty() {
        return Err(HistoryError::Ambiguous(simulation.diagnostics));
    }
    let id = sha256(&serde_json::to_vec(&fixed)?);
    store.fact(&format!("scenario:{id}"), &fixed, now)?;
    Ok(id)
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Status {
    pub catalog: Option<Catalog>,
    pub laws: Vec<Coverage>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn status(store: &impl Store, known_at: Option<DateTime<Utc>>) -> Result<Status> {
    let comparisons = store.facts_with_prefix::<crate::reconstruction::ComparisonResult>(
        "comparison-result:",
        known_at,
    )?;
    Ok(Status {
        catalog: store.latest_fact("catalog", known_at)?,
        laws: store
            .law_ids()?
            .iter()
            .map(|law| coverage(store, law, known_at))
            .collect::<Result<_>>()?,
        diagnostics: comparisons
            .into_iter()
            .flat_map(|result| result.diagnostics)
            .collect(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ParliamentaryContext {
    pub bills: Vec<Bill>,
    pub speeches: Vec<Speech>,
}

pub fn parliament(
    store: &impl Store,
    law: &str,
    known_at: Option<DateTime<Utc>>,
) -> Result<ParliamentaryContext> {
    let bills: Vec<Bill> = store
        .facts_with_prefix::<Bill>("bill:", known_at)?
        .into_iter()
        .filter(|bill| bill.enacted_law_ids.iter().any(|id| id == law))
        .collect();
    let ids: BTreeSet<_> = bills
        .iter()
        .flat_map(|bill| bill.speeches.iter().map(|link| link.speech_id.clone()))
        .collect();
    let speeches = store
        .facts_with_prefix::<Speech>("speech:", known_at)?
        .into_iter()
        .filter(|speech| ids.contains(&speech.speech_id))
        .collect();
    Ok(ParliamentaryContext { bills, speeches })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn export_snapshot(
    store: &impl Store,
    directory: &Path,
    laws: &[String],
    query: &Query,
    today: Day,
) -> Result<BTreeMap<String, String>> {
    if directory.exists() && fs::read_dir(directory)?.next().is_some() {
        return Err(HistoryError::Invalid(
            "The export destination must be empty".into(),
        ));
    }
    let ids = if laws.is_empty() {
        store.law_ids()?
    } else {
        laws.to_vec()
    };
    let mut selected = Vec::new();
    for law in &ids {
        selected.push((law.clone(), select(store, law, query, today)?));
    }
    fs::create_dir_all(directory)?;
    fs::write(
        directory.join("selection.json"),
        serde_json::to_vec_pretty(query)?,
    )?;
    for (law, item) in &selected {
        let law_dir = directory.join(law);
        fs::create_dir(&law_dir)?;
        fs::write(
            law_dir.join("text.txt"),
            xml::render_text(&item.selected.revision.tree),
        )?;
        fs::write(
            law_dir.join("selected.json"),
            serde_json::to_vec_pretty(item)?,
        )?;
        fs::write(
            law_dir.join("history.json"),
            serde_json::to_vec_pretty(&store.history(law, query.known_at)?)?,
        )?;
    }
    let mut files = BTreeMap::new();
    files.insert(
        "selection.json".into(),
        sha256(&fs::read(directory.join("selection.json"))?),
    );
    for (law, _) in &selected {
        for name in ["text.txt", "selected.json", "history.json"] {
            let path = format!("{law}/{name}");
            files.insert(path.clone(), sha256(&fs::read(directory.join(&path))?));
        }
    }
    Ok(files)
}

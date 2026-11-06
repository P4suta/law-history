use crate::{
    model::*,
    references::{NUMERAL_PATTERN, reference_number},
    store::sha256,
    xml,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use utoipa::ToSchema;

const MAXIMUM_SCENARIO_STEPS: usize = 256;
const MAXIMUM_SCENARIO_OPERATIONS: usize = 10_000;
const FIRST_ORDINAL: usize = 1;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    ReplaceText {
        law_id: String,
        path: String,
        old: String,
        new: String,
    },
    Replace {
        law_id: String,
        path: String,
        expected: Element,
        replacement: Element,
    },
    Insert {
        law_id: String,
        parent: String,
        before: Option<String>,
        element: Element,
    },
    Delete {
        law_id: String,
        path: String,
        expected: Element,
    },
    Move {
        law_id: String,
        path: String,
        expected: Element,
        parent: String,
        before: Option<String>,
    },
    Renumber {
        law_id: String,
        path: String,
        from: String,
        to: String,
        title: String,
    },
    Repeal {
        law_id: String,
    },
}

impl Operation {
    pub fn law_id(&self) -> &str {
        match self {
            Self::ReplaceText { law_id, .. }
            | Self::Replace { law_id, .. }
            | Self::Insert { law_id, .. }
            | Self::Delete { law_id, .. }
            | Self::Move { law_id, .. }
            | Self::Renumber { law_id, .. }
            | Self::Repeal { law_id } => law_id,
        }
    }
    pub fn path(&self) -> &str {
        match self {
            Self::ReplaceText { path, .. }
            | Self::Replace { path, .. }
            | Self::Delete { path, .. }
            | Self::Move { path, .. }
            | Self::Renumber { path, .. } => path,
            Self::Insert { parent, .. } => parent,
            Self::Repeal { .. } => "/Law",
        }
    }
    fn paths(&self) -> Vec<&str> {
        match self {
            Self::Move { path, parent, .. } => vec![path, parent],
            _ => vec![self.path()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourcedOperation {
    pub operation: Operation,
    pub evidence: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Adjustment {
    pub target_step: String,
    pub expected_operations_sha256: String,
    pub replacement: Vec<SourcedOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AmendmentStep {
    pub id: String,
    pub effective: LegalDate,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub operations: Vec<SourcedOperation>,
    #[serde(default)]
    pub adjustments: Vec<Adjustment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PinnedReferenceContext {
    pub versions: BTreeMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub schema_version: u32,
    pub name: String,
    pub bases: BTreeMap<String, String>,
    pub at: Day,
    pub known_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_context: Option<PinnedReferenceContext>,
    pub steps: Vec<AmendmentStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Simulation {
    pub scenario_id: String,
    pub applied_steps: Vec<String>,
    pub results: BTreeMap<String, Revision>,
    pub diffs: Vec<Diff>,
    pub diagnostics: Vec<Diagnostic>,
    pub impact: crate::references::Impact,
}

fn diagnostic(code: DiagnosticCode, message: impl Into<String>, source: &Provenance) -> Diagnostic {
    Diagnostic::new(code, message, &source.url, &source.locator)
}

pub fn apply_operation(
    tree: &Element,
    item: &SourcedOperation,
) -> std::result::Result<Element, Diagnostic> {
    let nodes = xml::flatten(tree);
    let failure = |code, message: String| diagnostic(code, message, &item.evidence);
    let target = || {
        nodes.get(item.operation.path()).ok_or_else(|| {
            failure(
                DiagnosticCode::OriginalMismatch,
                format!("No target at {}", item.operation.path()),
            )
        })
    };
    let ensure = |actual: &Element, expected: &Element| {
        if law_domain::patch_decision(true, actual == expected, false)
            == law_domain::PatchDecision::Apply
        {
            Ok(())
        } else {
            Err(failure(
                DiagnosticCode::OriginalMismatch,
                "Original element does not match the pinned source".into(),
            ))
        }
    };
    let result = match &item.operation {
        Operation::ReplaceText { path, old, new, .. } => {
            let actual = target()?;
            if law_domain::unique_match(&actual.text(), old).is_err() {
                return Err(failure(
                    DiagnosticCode::OriginalMismatch,
                    "Replacement requires exactly one occurrence in the target".into(),
                ));
            }
            fn edit(
                node: &Element,
                old: &str,
                new: &str,
            ) -> std::result::Result<(Element, usize), law_domain::TextEditError> {
                let mut result = node.clone();
                let mut count = 0;
                for part in &mut result.content {
                    match part {
                        Content::Text(text) => match law_domain::replace_unique(text, old, new) {
                            Ok(changed) => {
                                count += 1;
                                *text = changed;
                            }
                            Err(law_domain::TextEditError::MissingOriginal) => {}
                            Err(error) => return Err(error),
                        },
                        Content::Element(child) => {
                            let (changed, matches) = edit(child, old, new)?;
                            *child = changed;
                            count += matches;
                        }
                    }
                }
                Ok((result, count))
            }
            let (changed, count) = edit(actual, old, new).map_err(|error| {
                failure(
                    DiagnosticCode::InvalidInput,
                    format!("Text replacement was refused: {error:?}"),
                )
            })?;
            if count != 1 {
                return Err(failure(
                    DiagnosticCode::UnsupportedSyntax,
                    "Replacement spans XML element boundaries".into(),
                ));
            }
            xml::replace(tree, path, Some(&changed))
        }
        Operation::Replace {
            path,
            expected,
            replacement,
            ..
        } => {
            ensure(target()?, expected)?;
            xml::replace(tree, path, Some(replacement))
        }
        Operation::Delete { path, expected, .. } => {
            ensure(target()?, expected)?;
            xml::replace(tree, path, None)
        }
        Operation::Insert {
            parent,
            before,
            element,
            ..
        } => {
            ensure_unique_child(nodes.get(parent), element, None)
                .map_err(|error| failure(DiagnosticCode::Conflict, error))?;
            xml::append(tree, parent, element, before.as_deref())
        }
        Operation::Move {
            path,
            expected,
            parent,
            before,
            ..
        } => {
            ensure(target()?, expected)?;
            if parent == path || parent.starts_with(&format!("{path}/")) {
                return Err(failure(
                    DiagnosticCode::Conflict,
                    "An element cannot move into its own subtree".into(),
                ));
            }
            ensure_unique_child(nodes.get(parent), expected, Some(path))
                .map_err(|error| failure(DiagnosticCode::Conflict, error))?;
            xml::move_element(tree, path, parent, before.as_deref())
        }
        Operation::Renumber {
            path,
            from,
            to,
            title,
            ..
        } => {
            let mut changed = target()?.clone();
            if changed.attributes.get("Num") != Some(from)
                || to.is_empty()
                || !to.bytes().all(|byte| byte.is_ascii_digit() || byte == b'_')
            {
                return Err(failure(
                    DiagnosticCode::OriginalMismatch,
                    "Renumbering requires the original number and a valid destination".into(),
                ));
            }
            changed.attributes.insert("Num".into(), to.clone());
            let title_tag = format!("{}Title", changed.tag);
            for content in &mut changed.content {
                if let Content::Element(child) = content
                    && (child.tag == title_tag
                        || (changed.tag == "Paragraph" && child.tag == "ParagraphNum"))
                {
                    if title.is_empty() {
                        return Err(failure(
                            DiagnosticCode::InvalidInput,
                            "Renumbering requires the exact replacement title".into(),
                        ));
                    }
                    child.content = vec![Content::Text(title.clone())];
                }
            }
            let parent = path
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .unwrap_or_default();
            ensure_unique_child(nodes.get(parent), &changed, None)
                .map_err(|error| failure(DiagnosticCode::Conflict, error))?;
            xml::replace(tree, path, Some(&changed))
        }
        Operation::Repeal { .. } => Ok(tree.clone()),
    };
    let changed = result.map_err(|message| failure(DiagnosticCode::Conflict, message))?;
    if changed.tag != "Law" {
        return Err(failure(
            DiagnosticCode::Conflict,
            "An operation must preserve the Law root".into(),
        ));
    }
    xml::validate_tree(&changed).map_err(|message| failure(DiagnosticCode::Conflict, message))?;
    Ok(changed)
}

fn ensure_unique_child(
    parent: Option<&Element>,
    element: &Element,
    excluding: Option<&str>,
) -> std::result::Result<(), String> {
    let parent = parent.ok_or("Missing destination parent")?;
    if excluding.is_none()
        && xml::address_number(element).is_some_and(|number| {
            parent
                .children()
                .any(|child| child.tag == element.tag && xml::address_number(child) == Some(number))
        })
    {
        return Err("Destination number already exists".into());
    }
    Ok(())
}

pub fn simulate(scenario: &Scenario, bases: &BTreeMap<String, Revision>) -> Simulation {
    let scenario_id = sha256(&serde_json::to_vec(scenario).unwrap_or_default());
    let mut output = Simulation {
        scenario_id,
        applied_steps: Vec::new(),
        results: BTreeMap::new(),
        diffs: Vec::new(),
        diagnostics: Vec::new(),
        impact: crate::references::Impact::default(),
    };
    if scenario.steps.len() > MAXIMUM_SCENARIO_STEPS
        || scenario
            .steps
            .iter()
            .map(|step| step.operations.len() + step.adjustments.len())
            .sum::<usize>()
            > MAXIMUM_SCENARIO_OPERATIONS
    {
        output.diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidInput,
            "Scenario exceeds 256 steps or 10000 operations",
            "scenario",
            "/steps",
        ));
        return output;
    }
    let invalid = |message: &str| {
        Diagnostic::new(DiagnosticCode::InvalidInput, message, "scenario", "/steps")
    };
    if scenario.schema_version != SCHEMA_VERSION
        || scenario.name.trim().is_empty()
        || scenario.bases.is_empty()
    {
        output.diagnostics.push(invalid(
            "A supported schema, name, and pinned base versions are required",
        ));
        return output;
    }
    if bases.keys().ne(scenario.bases.keys()) {
        output
            .diagnostics
            .push(invalid("Base law set differs from the pinned law set"));
        return output;
    }
    for (law, revision) in bases {
        let actual = sha256(&serde_json::to_vec(revision).unwrap_or_default());
        if law_domain::patch_decision(
            scenario.bases.get(law) == Some(&actual) && &revision.law_id == law,
            true,
            false,
        ) != law_domain::PatchDecision::Apply
        {
            output.diagnostics.push(Diagnostic::new(
                DiagnosticCode::WrongBase,
                "The base content hash does not match",
                "scenario",
                law,
            ));
        }
    }
    let mut steps = scenario.steps.clone();
    let mut ids = BTreeSet::new();
    for step in &steps {
        if !ids.insert(step.id.clone()) || validate_id(&step.id).is_err() {
            output
                .diagnostics
                .push(invalid("Step identifiers must be valid and unique"));
        }
        if step.effective.day().is_none() {
            output.diagnostics.push(Diagnostic::new(
                DiagnosticCode::UnknownDate,
                "An unresolved enforcement date prevents deterministic simulation",
                "scenario",
                &step.id,
            ));
        }
    }
    for step in &steps {
        for dependency in &step.after {
            if !ids.contains(dependency)
                || steps
                    .iter()
                    .find(|item| &item.id == dependency)
                    .is_some_and(|item| item.effective.day() > step.effective.day())
            {
                output.diagnostics.push(invalid(
                    "A dependency is missing or takes effect after its dependent step",
                ));
            }
        }
    }
    if !output.diagnostics.is_empty() {
        return output;
    }
    let mut pending: Vec<_> = std::mem::take(&mut steps);
    let mut ordered: Vec<AmendmentStep> = Vec::new();
    while !pending.is_empty() {
        pending.sort_by(|a, b| (a.effective.day(), &a.id).cmp(&(b.effective.day(), &b.id)));
        let eligible = pending.iter().position(|step| {
            step.after
                .iter()
                .all(|id| ordered.iter().any(|prior| &prior.id == id))
        });
        let Some(index) = eligible else {
            output.diagnostics.push(invalid(
                "Amendment dependency cycle or unavailable predecessor",
            ));
            return output;
        };
        let step = pending.remove(index);
        ordered.push(step);
    }
    for index in 0..ordered.len() {
        if ordered[index]
            .effective
            .day()
            .is_some_and(|date| date > scenario.at)
        {
            continue;
        }
        let adjustments = ordered[index].adjustments.clone();
        for adjustment in adjustments {
            let Some(target) = ordered
                .iter()
                .position(|step| step.id == adjustment.target_step)
            else {
                output
                    .diagnostics
                    .push(invalid("Adjustment target is not an active step"));
                return output;
            };
            if target <= index
                || !ordered[target].after.contains(&ordered[index].id)
                || sha256(&serde_json::to_vec(&ordered[target].operations).unwrap_or_default())
                    != adjustment.expected_operations_sha256
            {
                output.diagnostics.push(invalid("Adjustment requires an unapplied, explicitly dependent step and matching original operations"));
                return output;
            }
            ordered[target].operations = adjustment.replacement;
        }
    }
    for (index, first) in ordered.iter().enumerate() {
        if first.effective.day().is_some_and(|date| date > scenario.at) {
            continue;
        }
        for second in ordered.iter().skip(index + 1) {
            if second
                .effective
                .day()
                .is_some_and(|date| date > scenario.at)
            {
                continue;
            }
            fn depends(
                step: &AmendmentStep,
                predecessor: &str,
                steps: &[AmendmentStep],
                seen: &mut BTreeSet<String>,
            ) -> bool {
                step.after.iter().any(|id| {
                    id == predecessor
                        || (seen.insert(id.clone())
                            && steps
                                .iter()
                                .find(|item| &item.id == id)
                                .is_some_and(|item| depends(item, predecessor, steps, seen)))
                })
            }
            if first.effective.day() == second.effective.day()
                && !depends(second, &first.id, &ordered, &mut BTreeSet::new())
            {
                let overlaps = first.operations.iter().any(|a| {
                    second.operations.iter().any(|b| {
                        a.operation.law_id() == b.operation.law_id()
                            && a.operation.paths().iter().any(|ap| {
                                b.operation.paths().iter().any(|bp| {
                                    ap == bp
                                        || ap.starts_with(&format!("{bp}/"))
                                        || bp.starts_with(&format!("{ap}/"))
                                })
                            })
                    })
                });
                if overlaps {
                    output.diagnostics.push(Diagnostic::new(
                        DiagnosticCode::AmbiguousOrder,
                        "Same-day overlapping amendments need an evidenced dependency",
                        "scenario",
                        &second.id,
                    ));
                    return output;
                }
            }
        }
    }
    let mut working = bases.clone();
    for step in &ordered {
        if step.effective.day().is_some_and(|date| date > scenario.at) {
            continue;
        }
        for item in &step.operations {
            let Some(revision) = working.get_mut(item.operation.law_id()) else {
                output.diagnostics.push(diagnostic(
                    DiagnosticCode::WrongBase,
                    "The target law has no pinned base",
                    &item.evidence,
                ));
                return output;
            };
            match apply_operation(&revision.tree, item) {
                Ok(tree) => {
                    revision.tree = tree;
                    revision.effective = step.effective.clone();
                    if let Operation::Repeal { .. } = item.operation {
                        revision.repeal = step.effective.day();
                    }
                }
                Err(error) => {
                    output.diagnostics.push(error);
                    output.applied_steps.clear();
                    return output;
                }
            }
        }
    }
    for (law, revision) in &working {
        if let Some(base) = bases.get(law) {
            let mut diff = structural_diff(
                &base.tree,
                &revision.tree,
                &scenario.bases[law],
                &output.scenario_id,
            );
            attach_relationships(&mut diff, &base.tree, &revision.tree, law, &ordered);
            output.diffs.push(diff);
        }
    }
    for (law, revision) in &mut working {
        let changed = bases.get(law).is_some_and(|base| {
            law_domain::revision_has_changes(
                base.tree != revision.tree,
                base.repeal != revision.repeal,
                base.effective != revision.effective,
            )
        });
        if changed {
            revision.revision_id = format!("scenario_{}", output.scenario_id);
            revision.kind = RevisionKind::Amendment;
            revision.amendment_promulgated = None;
            revision.metadata_source = None;
            revision.original_enactment = false;
            revision.source.confidence = Confidence::Inferred;
            revision.source.locator = format!(
                "scenario:{};base:{}",
                output.scenario_id, scenario.bases[law]
            );
        }
    }
    output.applied_steps = ordered
        .iter()
        .filter(|step| step.effective.day().is_some_and(|date| date <= scenario.at))
        .map(|step| step.id.clone())
        .collect();
    output.results = working;
    output
}

fn attach_relationships(
    diff: &mut Diff,
    before: &Element,
    after: &Element,
    law: &str,
    steps: &[AmendmentStep],
) {
    let left = xml::index(before);
    let right = xml::index(after);
    for item in steps
        .iter()
        .flat_map(|step| &step.operations)
        .filter(|item| item.operation.law_id() == law)
    {
        let (path, destination, kind) = match &item.operation {
            Operation::Renumber { path, to, .. } => {
                let Some(original) = left.get(path) else {
                    continue;
                };
                let Some((parent, _)) = path.rsplit_once('/') else {
                    continue;
                };
                let mut renamed = (*original).clone();
                renamed.attributes.insert("Num".into(), to.clone());
                (
                    path,
                    format!("{parent}/{}", xml::segment(&renamed, FIRST_ORDINAL)),
                    ChangeKind::Renumbered,
                )
            }
            Operation::Move {
                path,
                parent,
                expected,
                ..
            } => {
                let candidates: Vec<_> = right
                    .iter()
                    .filter(|(candidate, node)| {
                        candidate
                            .rsplit_once('/')
                            .is_some_and(|(prefix, _)| prefix == parent)
                            && **node == expected
                    })
                    .map(|(path, _)| path.clone())
                    .collect();
                if candidates.len() != 1 {
                    continue;
                }
                (path, candidates[0].clone(), ChangeKind::Moved)
            }
            _ => continue,
        };
        if let (Some(original), Some(changed)) = (left.get(path), right.get(&destination)) {
            diff.changes.retain(|change| {
                !change.before.as_ref().is_some_and(|node| {
                    node.path == *path || node.path.starts_with(&format!("{path}/"))
                }) && !change.after.as_ref().is_some_and(|node| {
                    node.path == destination || node.path.starts_with(&format!("{destination}/"))
                })
            });
            diff.changes.push(Change {
                kind,
                before: Some(LocatedElement {
                    path: path.clone(),
                    element: (*original).clone(),
                }),
                after: Some(LocatedElement {
                    path: destination,
                    element: (*changed).clone(),
                }),
                evidence: Some(item.evidence.clone()),
            });
        }
    }
}

pub fn structural_diff(before: &Element, after: &Element, from: &str, to: &str) -> Diff {
    let left = xml::flatten(before);
    let right = xml::flatten(after);
    let mut changes = Vec::new();
    let mut parents = Vec::<String>::new();
    let keys: BTreeSet<_> = left.keys().chain(right.keys()).cloned().collect();
    for path in keys {
        if parents
            .iter()
            .any(|parent| path.starts_with(&format!("{parent}/")))
        {
            continue;
        }
        let original = left.get(&path);
        let changed = right.get(&path);
        if original == changed {
            continue;
        }
        let (kind, include) = match (original, changed) {
            (Some(a), Some(b)) => {
                let direct_a: Vec<_> = a
                    .content
                    .iter()
                    .filter(|part| matches!(part, Content::Text(_)))
                    .collect();
                let direct_b: Vec<_> = b
                    .content
                    .iter()
                    .filter(|part| matches!(part, Content::Text(_)))
                    .collect();
                let order_a: Vec<_> = a
                    .children()
                    .map(|child| (&child.tag, child.attributes.get("Num")))
                    .collect();
                let order_b: Vec<_> = b
                    .children()
                    .map(|child| (&child.tag, child.attributes.get("Num")))
                    .collect();
                let mut sorted_a = order_a.clone();
                sorted_a.sort();
                let mut sorted_b = order_b.clone();
                sorted_b.sort();
                match law_domain::structural_change(
                    a.attributes == b.attributes,
                    direct_a == direct_b,
                    order_a == order_b,
                    sorted_a == sorted_b,
                ) {
                    law_domain::StructuralChange::Unchanged => (ChangeKind::Changed, false),
                    law_domain::StructuralChange::Changed => (ChangeKind::Changed, true),
                    law_domain::StructuralChange::Moved => (ChangeKind::Moved, true),
                }
            }
            (Some(_), None) => (ChangeKind::Removed, true),
            (None, Some(_)) => (ChangeKind::Added, true),
            (None, None) => continue,
        };
        if include {
            if matches!(kind, ChangeKind::Added | ChangeKind::Removed) {
                parents.push(path.clone());
            }
            changes.push(Change {
                kind,
                before: left.get(&path).cloned().map(|element| LocatedElement {
                    path: path.clone(),
                    element,
                }),
                after: right.get(&path).cloned().map(|element| LocatedElement {
                    path: path.clone(),
                    element,
                }),
                evidence: None,
            });
        }
    }
    Diff {
        from: from.into(),
        to: to.into(),
        changes,
        diagnostics: Vec::new(),
    }
}

pub fn parse_amending_sentence(
    text: &str,
    law_id: &str,
    tree: &Element,
    evidence: Provenance,
) -> std::result::Result<SourcedOperation, Diagnostic> {
    let unsupported = || {
        diagnostic(
            DiagnosticCode::UnsupportedSyntax,
            "Unsupported amending sentence; no operations were inferred",
            &evidence,
        )
    };
    let expression = regex::Regex::new(&format!(
        "^第(?P<article>{NUMERAL_PATTERN})条(?P<article_branches>(?:の{NUMERAL_PATTERN})*)(?:第(?P<paragraph>{NUMERAL_PATTERN})項)?(?:第(?P<item>{NUMERAL_PATTERN})号(?P<item_branches>(?:の{NUMERAL_PATTERN})*))?中「(?P<old>[^」]+)」を「(?P<new>[^」]*)」に改める。?$",
    ))
    .map_err(|_| unsupported())?;
    let captures = expression.captures(text.trim()).ok_or_else(unsupported)?;
    let number = reference_number(&captures["article"], &captures["article_branches"])
        .ok_or_else(unsupported)?;
    let nodes = xml::index(tree);
    let targets: Vec<_> = nodes
        .iter()
        .filter(|(path, node)| {
            node.attributes.get("Num") == Some(&number) && xml::is_main_article(path, &nodes)
        })
        .collect();
    if targets.len() != 1 {
        return Err(diagnostic(
            DiagnosticCode::OriginalMismatch,
            "The article locator is absent or ambiguous",
            &evidence,
        ));
    }
    let mut path = targets[0].0.clone();
    for (capture, tag, branches) in [
        ("paragraph", "Paragraph", None),
        ("item", "Item", Some("item_branches")),
    ] {
        if let Some(part) = captures.name(capture) {
            let number = reference_number(
                part.as_str(),
                branches
                    .and_then(|name| captures.name(name))
                    .map_or("", |part| part.as_str()),
            )
            .ok_or_else(unsupported)?;
            let candidates: Vec<_> = nodes
                .iter()
                .filter(|(candidate, element)| {
                    candidate
                        .rsplit_once('/')
                        .is_some_and(|(parent, _)| parent == path)
                        && element.tag == tag
                        && element.attributes.get("Num") == Some(&number)
                })
                .collect();
            if candidates.len() != 1 {
                return Err(diagnostic(
                    DiagnosticCode::OriginalMismatch,
                    "The paragraph or item locator is absent or ambiguous",
                    &evidence,
                ));
            }
            path = candidates[0].0.clone();
        }
    }
    Ok(SourcedOperation {
        operation: Operation::ReplaceText {
            law_id: law_id.into(),
            path,
            old: captures["old"].into(),
            new: captures["new"].into(),
        },
        evidence,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ParsedAmendment {
    pub operations: Vec<SourcedOperation>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse_document(
    input: &str,
    law_id: &str,
    tree: &Element,
    evidence: &Provenance,
) -> ParsedAmendment {
    let mut output = ParsedAmendment {
        operations: Vec::new(),
        diagnostics: Vec::new(),
    };
    let sources = if input.trim_start().starts_with('<') {
        match xml::parse_xml(input) {
            Ok(document) => xml::flatten(&document)
                .into_iter()
                .filter(|(_, element)| element.tag == "AmendProvision")
                .map(|(path, element)| {
                    let sentence = element
                        .children()
                        .find(|child| child.tag == "AmendProvisionSentence")
                        .map(Element::text)
                        .unwrap_or_default();
                    let replacements: Vec<_> = element
                        .children()
                        .filter(|child| child.tag == "NewProvision")
                        .flat_map(Element::children)
                        .cloned()
                        .collect();
                    (path, sentence, replacements)
                })
                .collect::<Vec<_>>(),
            Err(error) => {
                output
                    .diagnostics
                    .push(diagnostic(DiagnosticCode::InvalidInput, error, evidence));
                return output;
            }
        }
    } else {
        input
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(index, line)| {
                (
                    format!("line:{}", index + 1),
                    line.trim().into(),
                    Vec::new(),
                )
            })
            .collect()
    };
    for (locator, sentence, replacements) in sources {
        let source = Provenance {
            locator,
            ..evidence.clone()
        };
        let parsed = if replacements.is_empty() {
            parse_amending_sentence(&sentence, law_id, tree, source.clone())
                .or_else(|_| parse_article_operation(&sentence, law_id, tree, &source, &[]))
        } else {
            parse_article_operation(&sentence, law_id, tree, &source, &replacements)
        };
        match parsed {
            Ok(operation) => output.operations.push(operation),
            Err(error) => output.diagnostics.push(error),
        }
    }
    if output.operations.is_empty() && output.diagnostics.is_empty() {
        output.diagnostics.push(diagnostic(
            DiagnosticCode::UnsupportedSyntax,
            "No supported amendment provisions were found",
            evidence,
        ));
    }
    if !output.diagnostics.is_empty() {
        output.operations.clear();
    }
    output
}

fn parse_article_operation(
    sentence: &str,
    law: &str,
    tree: &Element,
    evidence: &Provenance,
    replacements: &[Element],
) -> std::result::Result<SourcedOperation, Diagnostic> {
    let failure = || {
        diagnostic(
            DiagnosticCode::UnsupportedSyntax,
            "The amendment needs an unsupported operation or missing structured replacement",
            evidence,
        )
    };
    let expression = regex::Regex::new(&format!("^第(?P<article>{NUMERAL_PATTERN})条(?P<article_branches>(?:の{NUMERAL_PATTERN})*)(?P<directive>を削る|を(?P<destination_title>第(?P<destination>{NUMERAL_PATTERN})条(?P<destination_branches>(?:の{NUMERAL_PATTERN})*))とする|を次のように改める|の次に次の一条を加える)。?$" )).map_err(|_| failure())?;
    let captures = expression.captures(sentence.trim()).ok_or_else(failure)?;
    let number = reference_number(&captures["article"], &captures["article_branches"])
        .ok_or_else(failure)?;
    let nodes = xml::index(tree);
    let targets: Vec<_> = nodes
        .iter()
        .filter(|(path, element)| {
            xml::is_main_article(path, &nodes) && element.attributes.get("Num") == Some(&number)
        })
        .collect();
    if targets.len() != 1 {
        return Err(diagnostic(
            DiagnosticCode::OriginalMismatch,
            "The original article is absent or ambiguous",
            evidence,
        ));
    }
    let (path, original) = targets[0];
    let operation = if &captures["directive"] == "を削る" {
        Operation::Delete {
            law_id: law.into(),
            path: path.clone(),
            expected: (**original).clone(),
        }
    } else if let Some(destination) = captures.name("destination") {
        Operation::Renumber {
            law_id: law.into(),
            path: path.clone(),
            from: number,
            to: reference_number(destination.as_str(), &captures["destination_branches"])
                .ok_or_else(failure)?,
            title: captures["destination_title"].into(),
        }
    } else if &captures["directive"] == "を次のように改める" && replacements.len() == 1 {
        Operation::Replace {
            law_id: law.into(),
            path: path.clone(),
            expected: (**original).clone(),
            replacement: replacements[0].clone(),
        }
    } else if &captures["directive"] == "の次に次の一条を加える" && replacements.len() == 1
    {
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .ok_or_else(failure)?;
        let parent_element = nodes.get(parent).ok_or_else(failure)?;
        let siblings = xml::child_paths(parent_element, parent);
        let index = siblings
            .iter()
            .position(|sibling| sibling == path)
            .ok_or_else(failure)?;
        Operation::Insert {
            law_id: law.into(),
            parent: parent.into(),
            before: siblings.get(index + 1).cloned(),
            element: replacements[0].clone(),
        }
    } else {
        return Err(failure());
    };
    Ok(SourcedOperation {
        operation,
        evidence: evidence.clone(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ComparisonRow {
    pub law_id: String,
    pub path: String,
    pub before: Option<Element>,
    pub after: Option<Element>,
    pub evidence: Provenance,
}

pub fn comparison_operations(
    rows: &[ComparisonRow],
) -> std::result::Result<Vec<SourcedOperation>, Diagnostic> {
    rows.iter()
        .map(|row| {
            let operation = match (&row.before, &row.after) {
                (Some(expected), Some(replacement)) => Operation::Replace {
                    law_id: row.law_id.clone(),
                    path: row.path.clone(),
                    expected: expected.clone(),
                    replacement: replacement.clone(),
                },
                (Some(expected), None) => Operation::Delete {
                    law_id: row.law_id.clone(),
                    path: row.path.clone(),
                    expected: expected.clone(),
                },
                (None, Some(element)) => Operation::Insert {
                    law_id: row.law_id.clone(),
                    parent: row.path.clone(),
                    before: None,
                    element: element.clone(),
                },
                (None, None) => {
                    return Err(diagnostic(
                        DiagnosticCode::InvalidInput,
                        "An empty comparison row has no operation",
                        &row.evidence,
                    ));
                }
            };
            Ok(SourcedOperation {
                operation,
                evidence: row.evidence.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_patch_preserves_input_and_refuses_ambiguous_matches() {
        for case in 0..4 {
            let content = match case {
                0 => vec![Content::Text("甲".into())],
                1 => vec![Content::Text("甲甲".into())],
                2 => vec![Content::Text("乙".into())],
                _ => vec![
                    Content::Text("甲".into()),
                    Content::Element(Element {
                        tag: "Ruby".into(),
                        attributes: BTreeMap::new(),
                        content: vec![Content::Text("乙".into())],
                    }),
                ],
            };
            let tree = Element {
                tag: "Law".into(),
                attributes: BTreeMap::new(),
                content,
            };
            let original = tree.clone();
            let item = SourcedOperation {
                operation: Operation::ReplaceText {
                    law_id: "LAW".into(),
                    path: "/Law".into(),
                    old: if case == 3 { "甲乙" } else { "甲" }.into(),
                    new: "新".into(),
                },
                evidence: Provenance {
                    url: "source".into(),
                    raw_sha256: String::new(),
                    media_type: "xml".into(),
                    attribution: "fixture".into(),
                    terms_url: "terms".into(),
                    confidence: Confidence::Synthetic,
                    locator: "/Law".into(),
                },
            };
            let result = apply_operation(&tree, &item);
            assert_eq!(tree, original);
            assert_eq!(result.is_ok(), case == 0);
            if let Ok(changed) = result {
                assert_eq!(changed.text(), "新");
            }
        }
    }
}

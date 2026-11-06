use crate::{model::*, xml};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use utoipa::ToSchema;

const NUMERAL_DIGITS: &[(char, u32)] = &[
    ('〇', 0),
    ('零', 0),
    ('一', 1),
    ('二', 2),
    ('三', 3),
    ('四', 4),
    ('五', 5),
    ('六', 6),
    ('七', 7),
    ('八', 8),
    ('九', 9),
];
const NUMERAL_UNITS: &[(char, u32)] = &[('十', 10), ('百', 100), ('千', 1000)];
const NUMERAL_UNIT_CEILING: u32 = 10_000;
pub(crate) const NUMERAL_PATTERN: &str = "[〇零一二三四五六七八九十百千0-9０-９]+";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
pub struct Position {
    pub law_id: String,
    pub version: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    Provision,
    Definition,
    Delegation,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Reference {
    pub from: Position,
    pub to: Option<Position>,
    pub text: String,
    pub kind: ReferenceKind,
    pub confidence: Confidence,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReferenceReport {
    pub references: Vec<Reference>,
    pub context_versions: BTreeMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReviewPath {
    pub position: Position,
    pub via: Vec<Reference>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Impact {
    pub context_versions: BTreeMap<String, String>,
    pub direct: Vec<Position>,
    pub review: Vec<ReviewPath>,
    pub broken: Vec<Reference>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn japanese_number(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    if let Some(digits) = value
        .chars()
        .map(law_domain::decimal_digit)
        .collect::<Option<Vec<_>>>()
    {
        return digits
            .into_iter()
            .try_fold(0u32, law_domain::append_decimal_digit);
    }
    let mut total = 0u32;
    let mut digit = None;
    let mut previous_unit = NUMERAL_UNIT_CEILING;
    for character in value.chars() {
        if let Some((_, number)) = NUMERAL_DIGITS
            .iter()
            .find(|(symbol, _)| *symbol == character)
        {
            if digit.is_some() {
                return None;
            }
            digit = Some(*number);
        } else {
            let (_, unit) = NUMERAL_UNITS
                .iter()
                .find(|(symbol, _)| *symbol == character)?;
            if *unit >= previous_unit {
                return None;
            }
            total = total.checked_add(digit.unwrap_or(1u32).checked_mul(*unit)?)?;
            previous_unit = *unit;
            digit = None;
        }
    }
    total.checked_add(digit.unwrap_or(0))
}

pub fn analyze(corpus: &BTreeMap<String, (String, Revision)>) -> Vec<Reference> {
    let titles = corpus
        .iter()
        .map(|(law, (_, revision))| (law.clone(), revision.title.clone()))
        .collect();
    analyze_with_titles(corpus, &titles)
}

pub(crate) fn reference_number(base: &str, branches: &str) -> Option<String> {
    let mut number = japanese_number(base)?.to_string();
    for branch in branches.split('の').skip(1) {
        number.push('_');
        number.push_str(&japanese_number(branch)?.to_string());
    }
    Some(number)
}

pub fn analyze_with_titles(
    corpus: &BTreeMap<String, (String, Revision)>,
    titles: &BTreeMap<String, String>,
) -> Vec<Reference> {
    let Ok(pattern) = regex::Regex::new(&format!(
        "第(?P<article>{NUMERAL_PATTERN})条(?P<article_branches>(?:の{NUMERAL_PATTERN})*)(?:第(?P<paragraph>{NUMERAL_PATTERN})項)?(?:第(?P<item>{NUMERAL_PATTERN})号(?P<item_branches>(?:の{NUMERAL_PATTERN})*))?"
    )) else {
        return Vec::new();
    };
    let mut edges = Vec::new();
    let indexes: BTreeMap<_, _> = corpus
        .iter()
        .map(|(law, (_, revision))| (law.clone(), xml::index(&revision.tree)))
        .collect();
    for (law_id, (version, revision)) in corpus {
        for (path, element) in &indexes[law_id] {
            if element.tag != "Sentence" {
                continue;
            }
            let text = xml::base_text(element);
            let from = Position {
                law_id: law_id.clone(),
                version: version.clone(),
                path: path.clone(),
            };
            for captures in pattern.captures_iter(&text) {
                let Some(full) = captures.get(0) else {
                    continue;
                };
                let prefix = text[..full.start()].trim_end_matches(['の', ' ', '　']);
                let external: Vec<_> = titles
                    .iter()
                    .filter(|(_, title)| prefix.ends_with(title.as_str()))
                    .collect();
                let target_law = if external.len() == 1 {
                    external[0].0
                } else {
                    law_id
                };
                let unknown_external = external.len() > 1
                    || (external.is_empty() && prefix.ends_with(['法', '令', '則', '例']));
                let article = reference_number(&captures["article"], &captures["article_branches"]);
                let target = article.and_then(|article| {
                    corpus.get(target_law).and_then(|(target_version, _)| {
                        let candidates: Vec<_> = indexes[target_law]
                            .iter()
                            .filter(|(candidate, node)| {
                                xml::is_main_article(candidate, &indexes[target_law])
                                    && node.attributes.get("Num") == Some(&article)
                            })
                            .collect();
                        if candidates.len() != 1 {
                            return None;
                        }
                        let mut target_path = candidates[0].0.clone();
                        for (capture, branches, tag) in [
                            ("paragraph", None, "Paragraph"),
                            ("item", Some("item_branches"), "Item"),
                        ] {
                            if let Some(part) = captures.name(capture) {
                                let suffix = branches
                                    .and_then(|name| captures.name(name))
                                    .map_or("", |part| part.as_str());
                                let number = reference_number(part.as_str(), suffix)?;
                                let matches: Vec<_> = indexes[target_law]
                                    .iter()
                                    .filter(|(candidate, node)| {
                                        candidate.starts_with(&format!("{target_path}/"))
                                            && node.tag == tag
                                            && node.attributes.get("Num") == Some(&number)
                                    })
                                    .collect();
                                if matches.len() != 1 {
                                    return None;
                                }
                                target_path = matches[0].0.clone();
                            }
                        }
                        Some(Position {
                            law_id: target_law.clone(),
                            version: target_version.clone(),
                            path: target_path,
                        })
                    })
                });
                let target = target.filter(|_| !unknown_external);
                let diagnostics = if target.is_none() {
                    vec![Diagnostic::new(
                        DiagnosticCode::UnresolvedReference,
                        "Reference target is absent or ambiguous in the selected corpus",
                        &revision.source.url,
                        path,
                    )]
                } else {
                    Vec::new()
                };
                edges.push(Reference {
                    from: from.clone(),
                    to: target,
                    text: full.as_str().into(),
                    kind: ReferenceKind::Provision,
                    confidence: Confidence::Inferred,
                    diagnostics,
                });
            }
            for marker in ["前条", "次条", "同条", "前項", "同項"] {
                if text.contains(marker) {
                    edges.push(Reference {
                        from: from.clone(),
                        to: None,
                        text: marker.into(),
                        kind: ReferenceKind::Provision,
                        confidence: Confidence::Inferred,
                        diagnostics: vec![Diagnostic::new(
                            DiagnosticCode::UnresolvedReference,
                            "Relative reference requires explicit contextual resolution",
                            &revision.source.url,
                            path,
                        )],
                    });
                }
            }
            if text.contains("とは、") || text.contains("をいう。") {
                edges.push(Reference {
                    from: from.clone(),
                    to: Some(from.clone()),
                    text: text.clone(),
                    kind: ReferenceKind::Definition,
                    confidence: Confidence::Inferred,
                    diagnostics: Vec::new(),
                });
            }
            if text.contains("政令で定める")
                || text.contains("省令で定める")
                || text.contains("規則で定める")
            {
                edges.push(Reference {
                    from: from.clone(),
                    to: None,
                    text: text.clone(),
                    kind: ReferenceKind::Delegation,
                    confidence: Confidence::Inferred,
                    diagnostics: vec![Diagnostic::new(
                        DiagnosticCode::UnresolvedReference,
                        "Delegated instrument is not identified by the provision text alone",
                        &revision.source.url,
                        path,
                    )],
                });
            }
        }
    }
    let mut seen = BTreeSet::new();
    edges.retain(|edge| {
        seen.insert((
            edge.from.clone(),
            edge.to.clone(),
            edge.kind.clone(),
            edge.text.clone(),
        ))
    });
    edges
}

fn intersects(first: &str, second: &str) -> bool {
    first == second
        || first.starts_with(&format!("{second}/"))
        || second.starts_with(&format!("{first}/"))
}

pub fn impact(edges: &[Reference], direct: Vec<Position>) -> Impact {
    let mut result = Impact {
        direct: direct.clone(),
        ..Default::default()
    };
    let mut unseen: BTreeSet<_> = edges
        .iter()
        .map(|edge| &edge.from)
        .filter(|position| !direct.contains(position))
        .collect();
    let mut queue: VecDeque<_> = direct
        .into_iter()
        .map(|position| (position, Vec::new()))
        .collect();
    while let Some((position, route)) = queue.pop_front() {
        for edge in edges {
            if edge.to.as_ref().is_some_and(|target| {
                target.law_id == position.law_id
                    && target.version == position.version
                    && intersects(&target.path, &position.path)
            }) {
                let Some(discovered) = unseen.take(&edge.from) else {
                    continue;
                };
                let mut via = route.clone();
                via.push(edge.clone());
                result.review.push(ReviewPath {
                    position: discovered.clone(),
                    via: via.clone(),
                });
                queue.push_back((discovered.clone(), via));
            }
        }
    }
    result.broken = edges
        .iter()
        .filter(|edge| edge.to.is_none())
        .cloned()
        .collect();
    result
}

use law_history::{
    Store,
    amendment::{self, AmendmentStep, Operation, Scenario, SourcedOperation},
    ingest,
    model::*,
    xml,
};
use std::collections::BTreeMap;

fn fixture() -> (Revision, Scenario) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    ingest::import_bundle(&store, include_bytes!("../../../fixtures/demo.json")).unwrap();
    let first = store.history("DEMO_ACT", None).unwrap().remove(0);
    let scenario = Scenario {
        schema_version: SCHEMA_VERSION,
        name: "Boundary contract".into(),
        bases: BTreeMap::from([("DEMO_ACT".into(), first.version)]),
        at: "1970-04-01".parse().unwrap(),
        known_at: None,
        reference_context: None,
        steps: Vec::new(),
    };
    (first.revision, scenario)
}

#[test]
fn scenario_identity_checks_each_changed_axis_without_a_coupled_enforcement_change() {
    for mask in 0..8 {
        let (mut base, mut scenario) = fixture();
        base.source.confidence = Confidence::Official;
        base.metadata_source = Some(base.source.clone());
        base.amendment_promulgated = Some(LegalDate::known(
            "1969-01-01".parse().unwrap(),
            "Fixture publication",
        ));
        scenario.bases.insert(
            base.law_id.clone(),
            law_history::store::sha256(&serde_json::to_vec(&base).unwrap()),
        );
        let nodes = xml::index(&base.tree);
        let (path, node) = nodes
            .iter()
            .find(|(_, node)| node.tag == "Sentence")
            .unwrap();
        let mut replacement = (*node).clone();
        if mask & 1 != 0 {
            replacement.content = vec![Content::Text("Changed content".into())];
        }
        let operation = sourced(
            &base,
            Operation::Replace {
                law_id: base.law_id.clone(),
                path: path.clone(),
                expected: (*node).clone(),
                replacement,
            },
        );
        let effective = if mask & 4 != 0 {
            LegalDate::known(scenario.at, "Changed enforcement")
        } else {
            base.effective.clone()
        };
        let mut first = step(&scenario, "content", vec![operation]);
        first.effective = effective.clone();
        scenario.steps.push(first);
        if mask & 2 != 0 {
            let mut repeal = step(
                &scenario,
                "repeal",
                vec![sourced(
                    &base,
                    Operation::Repeal {
                        law_id: base.law_id.clone(),
                    },
                )],
            );
            repeal.effective = effective;
            repeal.after.push("content".into());
            scenario.steps.push(repeal);
        }
        let result = amendment::simulate(
            &scenario,
            &BTreeMap::from([(base.law_id.clone(), base.clone())]),
        );
        assert!(
            result.diagnostics.is_empty(),
            "{mask}: {:?}",
            result.diagnostics
        );
        let actual = &result.results[&base.law_id];
        assert_eq!(actual.tree != base.tree, mask & 1 != 0);
        assert_eq!(actual.repeal != base.repeal, mask & 2 != 0);
        assert_eq!(actual.effective != base.effective, mask & 4 != 0);
        if mask == 0 {
            assert!(actual == &base);
        } else {
            assert_ne!(actual.revision_id, base.revision_id, "{mask}");
            assert_eq!(actual.kind, RevisionKind::Amendment);
            assert_eq!(actual.source.confidence, Confidence::Inferred);
            assert!(actual.metadata_source.is_none());
            assert!(actual.amendment_promulgated.is_none());
            assert!(!actual.original_enactment);
            assert!(actual.source.locator.starts_with("scenario:"));
        }
    }
}

fn sourced(revision: &Revision, operation: Operation) -> SourcedOperation {
    SourcedOperation {
        operation,
        evidence: revision.source.clone(),
    }
}

fn step(scenario: &Scenario, id: &str, operations: Vec<SourcedOperation>) -> AmendmentStep {
    AmendmentStep {
        id: id.into(),
        effective: LegalDate::known(scenario.at, "Synthetic fixture"),
        after: Vec::new(),
        operations,
        adjustments: Vec::new(),
    }
}

fn edit(revision: &Revision, path: &str, new: &str) -> SourcedOperation {
    let expected = xml::index(&revision.tree)[path].clone();
    let mut replacement = expected.clone();
    replacement.content = vec![Content::Text(new.into())];
    sourced(
        revision,
        Operation::Replace {
            law_id: revision.law_id.clone(),
            path: path.into(),
            expected,
            replacement,
        },
    )
}

#[test]
fn same_day_conflicts_include_both_move_locations_and_allow_disjoint_edits() {
    let (revision, mut scenario) = fixture();
    let nodes = xml::index(&revision.tree);
    let articles: Vec<_> = nodes
        .iter()
        .filter(|(_, node)| node.tag == "Article")
        .map(|(path, _)| path.clone())
        .collect();
    assert!(articles.len() >= 2);
    scenario.steps = vec![
        step(&scenario, "first", vec![edit(&revision, &articles[0], "A")]),
        step(
            &scenario,
            "second",
            vec![edit(&revision, &articles[1], "B")],
        ),
    ];
    let bases = BTreeMap::from([(revision.law_id.clone(), revision.clone())]);
    let result = amendment::simulate(&scenario, &bases);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(result.results[&revision.law_id].tree.text().contains('A'));
    assert!(result.results[&revision.law_id].tree.text().contains('B'));
    let first = &articles[0];
    let destination = articles[1].rsplit_once('/').unwrap().0;
    scenario.steps[0].operations = vec![sourced(
        &revision,
        Operation::Move {
            law_id: revision.law_id.clone(),
            path: first.clone(),
            expected: nodes[first].clone(),
            parent: destination.into(),
            before: None,
        },
    )];
    scenario.steps[1].operations = vec![edit(&revision, &articles[1], "B")];
    let refused = amendment::simulate(&scenario, &bases);
    assert_eq!(refused.diagnostics[0].code, DiagnosticCode::AmbiguousOrder);
    assert!(refused.results.is_empty());
    scenario.steps[1].after = vec!["first".into()];
    assert!(
        amendment::simulate(&scenario, &bases)
            .diagnostics
            .is_empty()
    );
}

#[test]
fn move_refuses_itself_and_descendants_with_source_diagnostics() {
    let (revision, _) = fixture();
    let nodes = xml::index(&revision.tree);
    let (path, article) = nodes
        .iter()
        .find(|(_, node)| node.tag == "Article")
        .unwrap();
    let child = nodes
        .keys()
        .find(|candidate| candidate.starts_with(&format!("{path}/")))
        .unwrap();
    for parent in [path.as_str(), child.as_str()] {
        let operation = sourced(
            &revision,
            Operation::Move {
                law_id: revision.law_id.clone(),
                path: path.clone(),
                expected: (*article).clone(),
                parent: parent.into(),
                before: None,
            },
        );
        let error = amendment::apply_operation(&revision.tree, &operation).unwrap_err();
        assert_eq!(error.code, DiagnosticCode::Conflict);
        assert!(error.message.contains("own subtree"));
        assert_eq!(error.source, revision.source.url);
    }
}

#[test]
fn moves_resolve_ordinal_sources_destinations_and_siblings_in_the_original_tree() {
    let (revision, _) = fixture();
    let tree = xml::parse_xml("<Law><AppdxTable Num=\"0\"><Sentence>A</Sentence></AppdxTable><AppdxTable Num=\"0\"><Sentence>B</Sentence></AppdxTable><AppdxTable Num=\"0\"><Sentence>C</Sentence></AppdxTable></Law>").unwrap();
    for source in 0..3 {
        for before in 0..=3 {
            let path = format!("/Law/AppdxTable[index={}]", source + 1);
            let before_path =
                (before < 3).then(|| format!("/Law/AppdxTable[index={}]", before + 1));
            let operation = sourced(
                &revision,
                Operation::Move {
                    law_id: revision.law_id.clone(),
                    path: path.clone(),
                    expected: xml::index(&tree)[&path].clone(),
                    parent: "/Law".into(),
                    before: before_path,
                },
            );
            let result = amendment::apply_operation(&tree, &operation).unwrap();
            let mut expected = vec!["A", "B", "C"];
            let moved = expected.remove(source);
            expected.insert(if before > source { before - 1 } else { before }, moved);
            assert_eq!(
                result.text(),
                expected.concat(),
                "source {source}, before {before}"
            );
            assert_eq!(tree.text(), "ABC");
        }
    }
    let tree = xml::parse_xml("<Law><SupplProvision><Sentence>A</Sentence></SupplProvision><SupplProvision><Sentence>B</Sentence></SupplProvision></Law>").unwrap();
    let path = "/Law/SupplProvision[index=1]";
    let operation = sourced(
        &revision,
        Operation::Move {
            law_id: revision.law_id.clone(),
            path: path.into(),
            expected: xml::index(&tree)[path].clone(),
            parent: "/Law/SupplProvision[index=2]".into(),
            before: Some("/Law/SupplProvision[index=2]/Sentence[index=1]".into()),
        },
    );
    let changed = amendment::apply_operation(&tree, &operation).unwrap();
    assert_eq!(changed.children().count(), 1);
    assert_eq!(changed.text(), "AB");
    let moved = xml::index(&changed);
    assert!(moved.contains_key("/Law/SupplProvision[index=1]/SupplProvision[index=1]"));
}

#[test]
fn renumbering_checks_original_destination_title_and_existing_siblings() {
    let (revision, _) = fixture();
    let nodes = xml::index(&revision.tree);
    let (path, article) = nodes
        .iter()
        .find(|(_, node)| node.tag == "Article")
        .unwrap();
    let original = &article.attributes["Num"];
    for (from, to, title, code, message) in [
        (
            "wrong",
            "19",
            "第十九条",
            DiagnosticCode::OriginalMismatch,
            "original number",
        ),
        (
            original.as_str(),
            "",
            "第十九条",
            DiagnosticCode::OriginalMismatch,
            "valid destination",
        ),
        (
            original.as_str(),
            "bad",
            "第十九条",
            DiagnosticCode::OriginalMismatch,
            "valid destination",
        ),
        (
            original.as_str(),
            "19",
            "",
            DiagnosticCode::InvalidInput,
            "exact replacement title",
        ),
        (
            original.as_str(),
            original.as_str(),
            "title",
            DiagnosticCode::Conflict,
            "Destination number",
        ),
    ] {
        let item = sourced(
            &revision,
            Operation::Renumber {
                law_id: revision.law_id.clone(),
                path: path.clone(),
                from: from.into(),
                to: to.into(),
                title: title.into(),
            },
        );
        let error = amendment::apply_operation(&revision.tree, &item).unwrap_err();
        assert_eq!(error.code, code);
        assert!(error.message.contains(message), "{}", error.message);
    }
    for destination in ["19", "19_2"] {
        let item = sourced(
            &revision,
            Operation::Renumber {
                law_id: revision.law_id.clone(),
                path: path.clone(),
                from: original.clone(),
                to: destination.into(),
                title: "Exact supplied title".into(),
            },
        );
        let changed = amendment::apply_operation(&revision.tree, &item).unwrap();
        let changed = xml::index(&changed)
            .into_values()
            .find(|node| {
                node.tag == "Article"
                    && node
                        .attributes
                        .get("Num")
                        .is_some_and(|number| number == destination)
            })
            .unwrap();
        assert_eq!(
            changed
                .children()
                .find(|node| node.tag == "ArticleTitle")
                .unwrap()
                .text(),
            "Exact supplied title"
        );
    }
    let paragraph = "<Law><Paragraph Num=\"1\"><ParagraphNum>1</ParagraphNum><Sentence>preserved</Sentence></Paragraph></Law>";
    let tree = xml::parse_xml(paragraph).unwrap();
    let item = sourced(
        &revision,
        Operation::Renumber {
            law_id: revision.law_id.clone(),
            path: "/Law/Paragraph[num=1]".into(),
            from: "1".into(),
            to: "2".into(),
            title: "2".into(),
        },
    );
    let changed = amendment::apply_operation(&tree, &item).unwrap();
    assert_eq!(changed.text(), "2preserved");
}

#[test]
fn insertion_distinguishes_duplicate_numbers_from_other_element_tags() {
    let (revision, _) = fixture();
    let tree = xml::parse_xml("<Law><Article Num=\"1\"/><Paragraph Num=\"2\"/></Law>").unwrap();
    for (element, accepted) in [
        ("<Article Num=\"1\"/>", false),
        ("<Article Num=\"2\"/>", true),
        ("<Item Num=\"1\"/>", true),
    ] {
        let item = sourced(
            &revision,
            Operation::Insert {
                law_id: revision.law_id.clone(),
                parent: "/Law".into(),
                before: None,
                element: xml::parse_xml(element).unwrap(),
            },
        );
        let result = amendment::apply_operation(&tree, &item);
        assert_eq!(result.is_ok(), accepted);
        if let Err(error) = result {
            assert!(error.message.contains("Destination number"));
        }
    }
}

#[test]
fn scenario_validation_refuses_each_invalid_identity_and_dependency() {
    let (revision, mut scenario) = fixture();
    scenario.steps = vec![
        step(&scenario, "first", Vec::new()),
        step(&scenario, "second", Vec::new()),
    ];
    let bases = BTreeMap::from([(revision.law_id.clone(), revision.clone())]);
    assert!(
        amendment::simulate(&scenario, &bases)
            .diagnostics
            .is_empty()
    );
    let mut invalid = Vec::new();
    let mut value = scenario.clone();
    value.schema_version += 1;
    invalid.push(value);
    let mut value = scenario.clone();
    value.name = " ".into();
    invalid.push(value);
    let mut value = scenario.clone();
    value.bases.clear();
    invalid.push(value);
    let mut value = scenario.clone();
    value.steps[1].id = "first".into();
    invalid.push(value);
    let mut value = scenario.clone();
    value.steps[1].id = "invalid/id".into();
    invalid.push(value);
    let mut value = scenario.clone();
    value.steps[1].after = vec!["missing".into()];
    invalid.push(value);
    let mut value = scenario.clone();
    value.steps[0].after = vec!["second".into()];
    value.steps[1].effective = LegalDate::known("1970-04-02".parse().unwrap(), "fixture");
    invalid.push(value);
    let mut value = scenario.clone();
    value.steps[0].after = vec!["second".into()];
    value.steps[1].after = vec!["first".into()];
    invalid.push(value);
    for value in invalid {
        let result = amendment::simulate(&value, &bases);
        assert!(!result.diagnostics.is_empty());
        assert!(result.results.is_empty());
        assert!(result.applied_steps.is_empty());
    }
    let mut altered = revision.clone();
    altered.law_id = "DIFFERENT".into();
    scenario.bases.insert(
        revision.law_id.clone(),
        law_history::store::sha256(&serde_json::to_vec(&altered).unwrap()),
    );
    let result = amendment::simulate(
        &scenario,
        &BTreeMap::from([(revision.law_id.clone(), altered)]),
    );
    assert_eq!(result.diagnostics[0].code, DiagnosticCode::WrongBase);
    assert!(result.results.is_empty());
}

#[test]
fn scenario_size_limits_are_inclusive_and_precede_semantic_validation() {
    let (revision, mut scenario) = fixture();
    scenario.schema_version = 0;
    let bases = BTreeMap::from([(revision.law_id.clone(), revision.clone())]);
    for (count, exceeds) in [(256, false), (257, true), (258, true)] {
        scenario.steps = (0..count)
            .map(|index| step(&scenario, &format!("step-{index}"), Vec::new()))
            .collect();
        let result = amendment::simulate(&scenario, &bases);
        assert_eq!(
            result.diagnostics[0].message.contains("Scenario exceeds"),
            exceeds
        );
    }
    for (count, exceeds) in [(10000, false), (10001, true), (10002, true)] {
        let operations = vec![
            sourced(
                &revision,
                Operation::Repeal {
                    law_id: revision.law_id.clone()
                }
            );
            count
        ];
        scenario.steps = vec![step(&scenario, "first", operations)];
        let result = amendment::simulate(&scenario, &bases);
        assert_eq!(
            result.diagnostics[0].message.contains("Scenario exceeds"),
            exceeds
        );
    }
}

#[test]
fn transitive_dependencies_and_future_steps_preserve_applied_history() {
    let (revision, mut scenario) = fixture();
    let path = xml::index(&revision.tree)
        .into_iter()
        .find(|(_, node)| node.tag == "Sentence" && node.text().contains("書面"))
        .unwrap()
        .0;
    let text_edit = |old: &str, new: &str| {
        sourced(
            &revision,
            Operation::ReplaceText {
                law_id: revision.law_id.clone(),
                path: path.clone(),
                old: old.into(),
                new: new.into(),
            },
        )
    };
    scenario.steps = vec![
        step(&scenario, "first", vec![text_edit("書面", "A")]),
        step(&scenario, "second", vec![text_edit("A", "B")]),
        step(&scenario, "third", vec![text_edit("B", "C")]),
    ];
    scenario.steps[1].after = vec!["first".into()];
    scenario.steps[2].after = vec!["second".into()];
    let bases = BTreeMap::from([(revision.law_id.clone(), revision.clone())]);
    let result = amendment::simulate(&scenario, &bases);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.applied_steps, ["first", "second", "third"]);
    assert!(result.results[&revision.law_id].tree.text().contains('C'));
    scenario.steps[2].effective = LegalDate::known("1970-04-02".parse().unwrap(), "fixture");
    let result = amendment::simulate(&scenario, &bases);
    assert!(result.diagnostics.is_empty());
    assert_eq!(result.applied_steps, ["first", "second"]);
    assert!(result.results[&revision.law_id].tree.text().contains('B'));
    assert!(!result.results[&revision.law_id].tree.text().contains('C'));
    scenario.at = "1970-03-31".parse().unwrap();
    let result = amendment::simulate(&scenario, &bases);
    assert!(result.diagnostics.is_empty());
    assert!(result.applied_steps.is_empty());
    assert_eq!(result.results[&revision.law_id], revision);
}

#[test]
fn renumber_relationships_replace_structural_pairs_and_retain_unrelated_edits() {
    let (revision, mut scenario) = fixture();
    let nodes = xml::index(&revision.tree);
    let articles: Vec<_> = nodes
        .iter()
        .filter(|(_, node)| node.tag == "Article")
        .collect();
    let (path, article) = articles[0];
    scenario.steps = vec![step(
        &scenario,
        "renumber",
        vec![
            sourced(
                &revision,
                Operation::Renumber {
                    law_id: revision.law_id.clone(),
                    path: path.clone(),
                    from: article.attributes["Num"].clone(),
                    to: "19".into(),
                    title: "第十九条".into(),
                },
            ),
            edit(&revision, articles[1].0, "Unrelated edit"),
        ],
    )];
    let result = amendment::simulate(
        &scenario,
        &BTreeMap::from([(revision.law_id.clone(), revision.clone())]),
    );
    assert!(result.diagnostics.is_empty());
    let changes = &result.diffs[0].changes;
    let relations: Vec<_> = changes
        .iter()
        .filter(|change| matches!(change.kind, ChangeKind::Renumbered))
        .collect();
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].before.as_ref().unwrap().path, *path);
    assert!(
        relations[0]
            .after
            .as_ref()
            .unwrap()
            .path
            .ends_with("Article[num=19]")
    );
    assert_eq!(
        relations[0].evidence.as_ref().unwrap().url,
        revision.source.url
    );
    assert!(
        !changes
            .iter()
            .filter(|change| !matches!(change.kind, ChangeKind::Renumbered))
            .any(|change| {
                change.before.as_ref().is_some_and(|node| {
                    node.path == *path || node.path.starts_with(&format!("{path}/"))
                }) || change
                    .after
                    .as_ref()
                    .is_some_and(|node| node.path.contains("Article[num=19]"))
            })
    );
    assert!(changes.iter().any(|change| {
        change
            .after
            .as_ref()
            .is_some_and(|node| node.element.text().contains("Unrelated edit"))
    }));
}

#[test]
fn structural_diff_separates_attributes_text_and_sibling_order() {
    for (before, after, kind) in [
        (
            "<Law Flag=\"a\"/>",
            "<Law Flag=\"b\"/>",
            ChangeKind::Changed,
        ),
        ("<Law>before</Law>", "<Law>after</Law>", ChangeKind::Changed),
        (
            "<Law><Article Num=\"1\"/><Article Num=\"2\"/></Law>",
            "<Law><Article Num=\"2\"/><Article Num=\"1\"/></Law>",
            ChangeKind::Moved,
        ),
    ] {
        let before = xml::parse_xml(before).unwrap();
        let after = xml::parse_xml(after).unwrap();
        assert!(
            amendment::structural_diff(&before, &before, "same", "same")
                .changes
                .is_empty()
        );
        let difference = amendment::structural_diff(&before, &after, "before", "after");
        assert_eq!(difference.changes.len(), 1);
        assert_eq!(
            std::mem::discriminant(&difference.changes[0].kind),
            std::mem::discriminant(&kind)
        );
        assert_eq!(difference.changes[0].before.as_ref().unwrap().path, "/Law");
    }
}

#[test]
fn same_day_adjustments_apply_at_the_boundary_and_validate_each_precondition() {
    let (revision, mut scenario) = fixture();
    let path = xml::index(&revision.tree)
        .into_iter()
        .find(|(_, node)| node.tag == "Sentence" && node.text().contains("書面"))
        .unwrap()
        .0;
    let operation = |new: &str| {
        sourced(
            &revision,
            Operation::ReplaceText {
                law_id: revision.law_id.clone(),
                path: path.clone(),
                old: "書面".into(),
                new: new.into(),
            },
        )
    };
    let mut target = step(&scenario, "target", vec![operation("A")]);
    target.after = vec!["adjust".into()];
    let mut adjusting = step(&scenario, "adjust", Vec::new());
    adjusting.adjustments = vec![amendment::Adjustment {
        target_step: "target".into(),
        expected_operations_sha256: law_history::store::sha256(
            &serde_json::to_vec(&target.operations).unwrap(),
        ),
        replacement: vec![operation("B")],
    }];
    scenario.steps = vec![adjusting, target];
    let bases = BTreeMap::from([(revision.law_id.clone(), revision.clone())]);
    let result = amendment::simulate(&scenario, &bases);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(result.results[&revision.law_id].tree.text().contains('B'));
    assert!(
        !xml::index(&result.results[&revision.law_id].tree)[&path]
            .text()
            .contains('A')
    );
    for variant in ["missing_target", "missing_dependency", "wrong_operations"] {
        let mut invalid = scenario.clone();
        match variant {
            "missing_target" => invalid.steps[0].adjustments[0].target_step = "absent".into(),
            "missing_dependency" => invalid.steps[1].after.clear(),
            _ => invalid.steps[0].adjustments[0].expected_operations_sha256 = "a".repeat(64),
        }
        let result = amendment::simulate(&invalid, &bases);
        assert_eq!(result.diagnostics[0].code, DiagnosticCode::InvalidInput);
        assert!(result.results.is_empty());
    }
    let mut future = scenario;
    for step in &mut future.steps {
        step.effective = LegalDate::known("1970-04-02".parse().unwrap(), "fixture");
    }
    future.steps[0].adjustments[0].expected_operations_sha256 =
        "future evidence not yet applicable".into();
    let result = amendment::simulate(&future, &bases);
    assert!(result.diagnostics.is_empty());
    assert!(result.applied_steps.is_empty());
    assert_eq!(result.results[&revision.law_id], revision);
}

#[test]
fn historical_conflicts_need_a_real_dependency_path() {
    let (revision, mut scenario) = fixture();
    let nodes = xml::index(&revision.tree);
    let path = nodes
        .iter()
        .find(|(_, node)| node.tag == "Sentence" && node.text().contains("書面"))
        .unwrap()
        .0
        .clone();
    let other = nodes
        .iter()
        .find(|(candidate, node)| node.tag == "Article" && !path.starts_with(candidate.as_str()))
        .unwrap()
        .0
        .clone();
    let text = |old: &str, new: &str| {
        sourced(
            &revision,
            Operation::ReplaceText {
                law_id: revision.law_id.clone(),
                path: path.clone(),
                old: old.into(),
                new: new.into(),
            },
        )
    };
    scenario.steps = vec![
        step(&scenario, "first", vec![text("書面", "A")]),
        step(
            &scenario,
            "second",
            vec![edit(&revision, &other, "Other provision")],
        ),
        step(&scenario, "third", vec![text("A", "C")]),
    ];
    scenario.steps[2].after = vec!["second".into()];
    scenario.at = "1970-04-02".parse().unwrap();
    let bases = BTreeMap::from([(revision.law_id.clone(), revision)]);
    let result = amendment::simulate(&scenario, &bases);
    assert_eq!(result.diagnostics[0].code, DiagnosticCode::AmbiguousOrder);
    scenario.steps[1].after = vec!["first".into()];
    let result = amendment::simulate(&scenario, &bases);
    assert!(result.diagnostics.is_empty());
    assert_eq!(result.applied_steps, ["first", "second", "third"]);
}

#[test]
fn enforcement_only_scenario_changes_do_not_retain_the_official_base_identity() {
    let (mut revision, mut scenario) = fixture();
    revision.source.confidence = Confidence::Official;
    revision.metadata_source = Some(revision.source.clone());
    revision.amendment_promulgated = Some(LegalDate::known(
        "1969-01-01".parse().unwrap(),
        "Fixture publication",
    ));
    let operation = amendment::parse_amending_sentence(
        "第一条中「書面」を「書面」に改める。",
        &revision.law_id,
        &revision.tree,
        revision.source.clone(),
    )
    .unwrap();
    scenario
        .steps
        .push(step(&scenario, "same-text", vec![operation]));
    scenario.bases.insert(
        revision.law_id.clone(),
        law_history::store::sha256(&serde_json::to_vec(&revision).unwrap()),
    );
    let result = amendment::simulate(
        &scenario,
        &BTreeMap::from([(revision.law_id.clone(), revision.clone())]),
    );
    assert!(result.diagnostics.is_empty());
    let changed = &result.results[&revision.law_id];
    assert_eq!(changed.tree, revision.tree);
    assert_ne!(changed.effective, revision.effective);
    assert_ne!(changed.revision_id, revision.revision_id);
    assert_eq!(changed.kind, RevisionKind::Amendment);
    assert_eq!(changed.source.confidence, Confidence::Inferred);
    assert!(!changed.original_enactment);
    assert!(changed.amendment_promulgated.is_none());
    assert!(changed.metadata_source.is_none());
}

#[test]
fn amending_locators_respect_article_paragraph_item_and_provision_boundaries() {
    let (revision, _) = fixture();
    let tree = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1\"><Paragraph Num=\"1\"><Item Num=\"1\"><Sentence>甲</Sentence></Item></Paragraph><Paragraph Num=\"2\"><Item Num=\"1\"><Sentence>別</Sentence></Item></Paragraph></Article><Article Num=\"2\"><Paragraph Num=\"1\"><Item Num=\"1\"><Sentence>他</Sentence></Item></Paragraph></Article></MainProvision><SupplProvision><Article Num=\"1\"><Paragraph Num=\"1\"><Item Num=\"1\"><Sentence>附則</Sentence></Item></Paragraph></Article></SupplProvision></LawBody></Law>").unwrap();
    let item = amendment::parse_amending_sentence(
        "第一条第一項第一号中「甲」を「乙」に改める。",
        &revision.law_id,
        &tree,
        revision.source.clone(),
    )
    .unwrap();
    assert_eq!(
        item.operation.path(),
        "/Law/LawBody[index=1]/MainProvision[index=1]/Article[num=1]/Paragraph[num=1]/Item[num=1]"
    );
    assert_eq!(
        amendment::apply_operation(&tree, &item).unwrap().text(),
        "乙別他附則"
    );
    for sentence in [
        "第一条第三項中「甲」を「乙」に改める。",
        "第一条第一項第二号中「甲」を「乙」に改める。",
        "第三条中「甲」を「乙」に改める。",
    ] {
        let error = amendment::parse_amending_sentence(
            sentence,
            &revision.law_id,
            &tree,
            revision.source.clone(),
        )
        .unwrap_err();
        assert_eq!(error.code, DiagnosticCode::OriginalMismatch);
    }
    for sentence in [
        "第一条第十百項中「甲」を「乙」に改める。",
        "第一条第一項第十百号中「甲」を「乙」に改める。",
        "第十百条中「甲」を「乙」に改める。",
    ] {
        assert_eq!(
            amendment::parse_amending_sentence(
                sentence,
                &revision.law_id,
                &tree,
                revision.source.clone()
            )
            .unwrap_err()
            .code,
            DiagnosticCode::UnsupportedSyntax
        );
    }
}

#[test]
fn amending_locators_keep_branch_numbers_and_exclude_quoted_provisions() {
    let (revision, _) = fixture();
    let tree = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1_2\"><Paragraph Num=\"1\"><Sentence>Prefix</Sentence></Paragraph></Article><Article Num=\"1_2_3\"><ArticleTitle>第一条の二の三</ArticleTitle><Paragraph Num=\"1\"><Item Num=\"1_2_3\"><Sentence>甲</Sentence></Item><Sentence><QuoteStruct><Article Num=\"1_2_3\"><Paragraph Num=\"1\"><Item Num=\"1_2_3\"><Sentence>Quoted</Sentence></Item></Paragraph></Article></QuoteStruct></Sentence></Paragraph></Article><Article Num=\"8\"/><Article Num=\"8\"/></MainProvision></LawBody></Law>").unwrap();
    for sentence in [
        "第一条の二の三第一項第一号の二の三中「甲」を「乙」に改める。",
        "第１条の２の３第１項第１号の２の３中「甲」を「乙」に改める。",
    ] {
        let operation = amendment::parse_amending_sentence(
            sentence,
            &revision.law_id,
            &tree,
            revision.source.clone(),
        )
        .unwrap();
        assert_eq!(
            operation.operation.path(),
            "/Law/LawBody[index=1]/MainProvision[index=1]/Article[num=1_2_3]/Paragraph[num=1]/Item[num=1_2_3]"
        );
        assert!(
            amendment::apply_operation(&tree, &operation)
                .unwrap()
                .text()
                .contains("乙")
        );
    }
    let renamed = amendment::parse_document(
        "第一条の二の三を第二条の四の五とする。",
        &revision.law_id,
        &tree,
        &revision.source,
    );
    assert!(renamed.diagnostics.is_empty());
    let changed = amendment::apply_operation(&tree, &renamed.operations[0]).unwrap();
    assert!(
        xml::index(&changed)
            .contains_key("/Law/LawBody[index=1]/MainProvision[index=1]/Article[num=2_4_5]")
    );
    assert!(changed.text().contains("第二条の四の五"));
    let inserted = amendment::parse_document(
        "<AmendProvision><AmendProvisionSentence>第一条の二の三の次に次の一条を加える。</AmendProvisionSentence><NewProvision><Article Num=\"7\"/></NewProvision></AmendProvision>",
        &revision.law_id,
        &tree,
        &revision.source,
    );
    assert!(inserted.diagnostics.is_empty());
    let changed = amendment::apply_operation(&tree, &inserted.operations[0]).unwrap();
    assert_eq!(
        xml::index(&changed)["/Law/LawBody[index=1]/MainProvision[index=1]"]
            .children()
            .map(|node| node.attributes["Num"].as_str())
            .collect::<Vec<_>>(),
        ["1_2", "1_2_3", "7", "8", "8"]
    );
    let nested = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1\"><Paragraph Num=\"1\"><Sentence><QuoteStruct><Paragraph Num=\"2\"><Sentence>甲</Sentence></Paragraph></QuoteStruct></Sentence></Paragraph></Article></MainProvision></LawBody></Law>").unwrap();
    assert!(
        amendment::parse_amending_sentence(
            "第一条第二項中「甲」を「乙」に改める。",
            &revision.law_id,
            &nested,
            revision.source
        )
        .is_err()
    );
}

#[test]
fn structured_article_edits_insert_before_the_next_sibling_and_refuse_wrong_payload_counts() {
    let (revision, _) = fixture();
    let tree = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1\"/><Article Num=\"0\"/><Article Num=\"2\"/><Article Num=\"3\"/></MainProvision><SupplProvision><Article Num=\"1\"/></SupplProvision></LawBody></Law>").unwrap();
    let parse = |sentence: &str, replacement: &str| {
        amendment::parse_document(
            &format!(
                "<AmendProvision><AmendProvisionSentence>{sentence}</AmendProvisionSentence><NewProvision>{replacement}</NewProvision></AmendProvision>"
            ),
            &revision.law_id,
            &tree,
            &revision.source,
        )
    };
    let inserted = parse(
        "第一条の次に次の一条を加える。",
        "<Article Num=\"1_2\"><Sentence>追加</Sentence></Article>",
    );
    assert!(
        inserted.diagnostics.is_empty(),
        "{:?}",
        inserted.diagnostics
    );
    let result = amendment::apply_operation(&tree, &inserted.operations[0]).unwrap();
    let numbers: Vec<_> = xml::index(&result)["/Law/LawBody[index=1]/MainProvision[index=1]"]
        .children()
        .map(|child| child.attributes["Num"].as_str())
        .collect();
    assert_eq!(numbers, ["1", "1_2", "0", "2", "3"]);
    for sentence in [
        "第一条を次のように改める。",
        "第一条の次に次の一条を加える。",
    ] {
        for replacement in ["", "<Article Num=\"4\"/><Article Num=\"5\"/>"] {
            let result = parse(sentence, replacement);
            assert!(!result.diagnostics.is_empty());
            assert!(result.operations.is_empty());
        }
    }
    for (sentence, expected) in [
        ("第二条を削る。", "delete"),
        ("第三条を第四条とする。", "renumber"),
    ] {
        let parsed = parse(sentence, "");
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(
            serde_json::to_value(&parsed.operations[0].operation).unwrap()["kind"],
            expected
        );
        assert!(amendment::apply_operation(&tree, &parsed.operations[0]).is_ok());
    }
}

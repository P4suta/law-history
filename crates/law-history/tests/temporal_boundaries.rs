use law_history::{Store, ingest, model::*, service, xml};
use std::collections::BTreeMap;

fn day(text: &str) -> Day {
    text.parse().unwrap()
}
fn instant(text: &str) -> chrono::DateTime<chrono::Utc> {
    text.parse().unwrap()
}
fn fixture() -> (tempfile::TempDir, Store, Revision) {
    let directory = tempfile::tempdir().unwrap();
    let input = Store::open(directory.path().join("input")).unwrap();
    ingest::import_bundle(&input, include_bytes!("../../../fixtures/demo.json")).unwrap();
    let mut template = input.history("DEMO_ACT", None).unwrap().remove(0).revision;
    let store = Store::open(directory.path().join("cases")).unwrap();
    store
        .preserve(&input.source(&template.source.raw_sha256).unwrap())
        .unwrap();
    template.law_id = "CASE".into();
    template.revision_id = "BASE".into();
    template.sequence = None;
    template.effective = LegalDate::known(day("1970-01-01"), "Fixture date");
    (directory, store, template)
}
fn query(at: &str) -> Query {
    Query {
        at: Some(day(at)),
        ..Default::default()
    }
}

#[test]
fn body_search_finds_ruby_base_words_and_preserves_reading_queries() {
    let (_directory, store, mut base) = fixture();
    base.title = "META_ONLY".into();
    let raw = "<Law><Sentence>激<Ruby>甚<Rt>じん</Rt></Ruby>災害</Sentence></Law>";
    base.tree = xml::parse_xml(raw).unwrap();
    base.source.raw_sha256 = store.preserve(raw.as_bytes()).unwrap();
    let version = store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    for rebuild in [false, true] {
        if rebuild {
            store.rebuild().unwrap();
        }
        for term in ["激甚災害", "じん"] {
            let response =
                service::search(&store, term, &Query::default(), day("1970-01-01")).unwrap();
            assert_eq!(response.hits.len(), 1, "{term}");
            assert_eq!(response.hits[0].version, version);
        }
    }
}

#[test]
fn current_state_snapshots_resolve_only_observed_matching_candidates_and_keep_past_ambiguity() {
    for (marker, acquired_at, at, expected) in [
        (
            Some("SECOND"),
            Some("2026-11-04T15:00:00Z"),
            "2026-11-05",
            Some("SECOND"),
        ),
        (
            Some("BASE"),
            Some("2026-11-04T15:00:00Z"),
            "2026-11-05",
            Some("BASE"),
        ),
        (
            Some("SECOND"),
            Some("2026-11-04T15:00:00Z"),
            "2026-11-06",
            Some("SECOND"),
        ),
        (
            Some("SECOND"),
            Some("2026-11-04T15:00:00Z"),
            "2026-11-04",
            None,
        ),
        (Some("SECOND"), None, "2026-11-05", None),
        (
            Some("ABSENT"),
            Some("2026-11-04T15:00:00Z"),
            "2026-11-05",
            None,
        ),
        (None, Some("2026-11-04T15:00:00Z"), "2026-11-05", None),
    ] {
        let (_directory, store, base) = fixture();
        let observed = instant("2026-01-01T00:00:00Z");
        let first = store.save_revision(&base, observed).unwrap();
        let mut second = base.clone();
        second.revision_id = "SECOND".into();
        let second_version = store.save_revision(&second, observed).unwrap();
        let recorded = instant("2026-11-05T00:00:00Z");
        let inventory = ingest::LawInventory {
            law_id: "CASE".into(),
            available_revisions: 2,
            revision_ids: vec!["BASE".into(), "SECOND".into()],
            collected_at: recorded,
            source_digest: base.source.raw_sha256.clone(),
            current_revision_id: marker.map(str::to_owned),
            current_snapshot_at: acquired_at.map(instant),
            current_version: Some(if marker == Some("BASE") {
                first.clone()
            } else {
                second_version
            }),
        };
        store.fact("inventory:CASE", &inventory, recorded).unwrap();
        for rebuild in [false, true] {
            if rebuild {
                store.rebuild().unwrap();
            }
            let result = service::select(&store, "CASE", &query(at), day("2026-11-05"));
            match expected {
                Some(revision) => {
                    assert_eq!(result.unwrap().selected.revision.revision_id, revision)
                }
                None => assert!(matches!(result, Err(HistoryError::Ambiguous(_)))),
            }
            let mut historical_knowledge = query(at);
            historical_knowledge.known_at = Some(recorded - chrono::Duration::nanoseconds(1));
            assert!(matches!(
                service::select(&store, "CASE", &historical_knowledge, day("2026-11-05")),
                Err(HistoryError::Ambiguous(_))
            ));
            let mut pinned = query(at);
            pinned.version = Some(first.clone());
            assert_eq!(
                service::select(&store, "CASE", &pinned, day("2026-11-05"))
                    .unwrap()
                    .selected
                    .version,
                first
            );
        }
    }
}

#[test]
fn current_state_evidence_refuses_wrong_identity_future_acquisition_and_sequence_conflicts() {
    for defect in ["law", "acquisition", "sequence"] {
        let (_directory, store, mut base) = fixture();
        base.sequence = Some(1);
        let first = store
            .save_revision(&base, instant("2026-01-01T00:00:00Z"))
            .unwrap();
        let mut second = base.clone();
        second.revision_id = "SECOND".into();
        second.sequence = Some(2);
        store
            .save_revision(&second, instant("2026-01-01T00:00:00Z"))
            .unwrap();
        let recorded = instant("2026-11-05T00:00:00Z");
        let inventory = ingest::LawInventory {
            law_id: if defect == "law" { "OTHER" } else { "CASE" }.into(),
            available_revisions: 2,
            revision_ids: vec!["BASE".into(), "SECOND".into()],
            collected_at: recorded,
            source_digest: base.source.raw_sha256,
            current_revision_id: Some("BASE".into()),
            current_version: Some(first),
            current_snapshot_at: Some(if defect == "acquisition" {
                recorded + chrono::Duration::nanoseconds(1)
            } else {
                recorded
            }),
        };
        store.fact("inventory:CASE", &inventory, recorded).unwrap();
        let error =
            service::select(&store, "CASE", &query("2026-11-05"), day("2026-11-05")).unwrap_err();
        if defect == "sequence" {
            assert!(matches!(error, HistoryError::Ambiguous(_)));
        } else {
            assert!(matches!(error, HistoryError::Invalid(_)));
        }
    }
}

#[test]
fn current_markers_bind_exact_content_instead_of_only_a_revision_identifier() {
    for defect in [
        "missing",
        "wrong_revision",
        "unknown_version",
        "later_correction",
    ] {
        let (_directory, store, base) = fixture();
        let first = store
            .save_revision(&base, instant("2026-01-01T00:00:00Z"))
            .unwrap();
        let mut second = base.clone();
        second.revision_id = "SECOND".into();
        let bound = store
            .save_revision(&second, instant("2026-01-01T00:00:00Z"))
            .unwrap();
        if defect == "later_correction" {
            second.title = "Corrected body".into();
            store
                .save_revision(&second, instant("2026-11-05T00:01:00Z"))
                .unwrap();
        }
        let recorded = instant("2026-11-05T00:00:00Z");
        let inventory = ingest::LawInventory {
            law_id: "CASE".into(),
            available_revisions: 2,
            revision_ids: vec!["BASE".into(), "SECOND".into()],
            collected_at: recorded,
            source_digest: base.source.raw_sha256,
            current_revision_id: Some("SECOND".into()),
            current_snapshot_at: Some(recorded),
            current_version: match defect {
                "missing" => None,
                "wrong_revision" => Some(first),
                "unknown_version" => Some("a".repeat(64)),
                _ => Some(bound),
            },
        };
        store.fact("inventory:CASE", &inventory, recorded).unwrap();
        for rebuild in [false, true] {
            if rebuild {
                store.rebuild().unwrap();
            }
            assert!(
                matches!(
                    service::select(&store, "CASE", &query("2026-11-05"), day("2026-11-05")),
                    Err(HistoryError::Ambiguous(_))
                ),
                "{defect}"
            );
        }
    }
}

#[test]
fn undated_repeal_records_from_any_producer_remain_inspectable_without_becoming_active() {
    let (_directory, store, mut revision) = fixture();
    revision.kind = RevisionKind::Repeal;
    revision.repeal = None;
    revision.termination = None;
    let version = store
        .save_revision(&revision, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    for rebuild in [false, true] {
        if rebuild {
            store.rebuild().unwrap();
        }
        assert!(matches!(
            service::select(&store, "CASE", &query("1970-01-01"), day("2026-11-05")),
            Err(HistoryError::Ambiguous(_))
        ));
        let mut pinned = query("1970-01-01");
        pinned.version = Some(version.clone());
        let inspected = service::select(&store, "CASE", &pinned, day("2026-11-05")).unwrap();
        assert_eq!(inspected.selected.version, version);
        assert_eq!(inspected.diagnostics[0].code, DiagnosticCode::UnknownDate);
    }
}

#[test]
fn confirmed_and_uncertain_revisions_keep_query_boundaries_and_diagnostics() {
    let (_directory, store, base) = fixture();
    let version = store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    for at in ["1970-01-01", "1970-01-02", "1970-01-03"] {
        assert_eq!(
            service::select(&store, "CASE", &query(at), day("1970-01-02"))
                .unwrap()
                .selected
                .version,
            version
        );
    }
    assert!(matches!(
        service::select(&store, "CASE", &query("1969-12-31"), day("1970-01-02")),
        Err(HistoryError::NotFound(_))
    ));
    let mut unknown = base.clone();
    unknown.revision_id = "PENDING".into();
    unknown.original_enactment = false;
    unknown.promulgated = LegalDate::known(day("1970-01-02"), "Fixture publication");
    unknown.effective = LegalDate::Uncertain {
        original: "Pending order".into(),
        reason: "Fixture uncertainty".into(),
    };
    store
        .save_revision(&unknown, instant("2026-01-02T00:00:00Z"))
        .unwrap();
    assert!(
        service::select(&store, "CASE", &query("1970-01-01"), day("1970-01-02"))
            .unwrap()
            .diagnostics
            .is_empty()
    );
    let current = service::select(&store, "CASE", &query("1970-01-02"), day("1970-01-02")).unwrap();
    assert_eq!(current.selected.version, version);
    assert_eq!(current.diagnostics[0].code, DiagnosticCode::UnknownDate);
    assert!(matches!(
        service::select(&store, "CASE", &query("1970-01-03"), day("1970-01-02")),
        Err(HistoryError::Ambiguous(_))
    ));
}

#[test]
fn latest_corrections_resolve_older_conflicts_but_preserve_their_knowledge_cutoff() {
    let (_directory, store, base) = fixture();
    let first = store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    for title in ["Correction one", "Correction two"] {
        let mut correction = base.clone();
        correction.title = title.into();
        store
            .save_revision(&correction, instant("2026-01-02T00:00:00Z"))
            .unwrap();
    }
    let mut at_conflict = query("1970-01-01");
    at_conflict.known_at = Some(instant("2026-01-02T00:00:00Z"));
    assert!(matches!(
        service::select(&store, "CASE", &at_conflict, day("2026-11-05")),
        Err(HistoryError::Ambiguous(_))
    ));
    let uncertain_coverage = service::coverage(&store, "CASE", at_conflict.known_at).unwrap();
    assert_eq!(uncertain_coverage.stored_revisions, 1);
    assert!(uncertain_coverage.earliest_known.is_none());
    assert!(!uncertain_coverage.enactment_known);
    assert!(
        uncertain_coverage
            .diagnostics
            .iter()
            .any(|item| item.code == DiagnosticCode::AmbiguousOrder)
    );
    store
        .save_revision(&base, instant("2026-01-03T00:00:00Z"))
        .unwrap();
    assert_eq!(
        service::select(&store, "CASE", &query("1970-01-01"), day("2026-11-05"))
            .unwrap()
            .selected
            .version,
        first
    );
    let mut explicit = query("1970-01-01");
    explicit.version = Some(first.clone());
    assert_eq!(
        service::select(&store, "CASE", &explicit, day("2026-11-05"))
            .unwrap()
            .selected
            .version,
        first
    );
    explicit.version = Some("a".repeat(64));
    assert!(matches!(
        service::select(&store, "CASE", &explicit, day("2026-11-05")),
        Err(HistoryError::NotFound(_))
    ));
}

#[test]
fn coverage_and_blame_use_latest_corrections_without_inventing_an_introduction() {
    let (_directory, store, mut base) = fixture();
    let source = "<Law><Sentence>A</Sentence></Law>";
    base.tree = xml::parse_xml(source).unwrap();
    base.source.raw_sha256 = store.preserve(source.as_bytes()).unwrap();
    let original = store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    let mut incorrect = base.clone();
    incorrect.original_enactment = false;
    incorrect.effective = LegalDate::known(day("1940-01-01"), "Superseded observation");
    let source = "<Law><Sentence>B</Sentence></Law>";
    incorrect.tree = xml::parse_xml(source).unwrap();
    incorrect.source.raw_sha256 = store.preserve(source.as_bytes()).unwrap();
    store
        .save_revision(&incorrect, instant("2026-01-02T00:00:00Z"))
        .unwrap();
    base.original_enactment = false;
    let current = store
        .save_revision(&base, instant("2026-01-03T00:00:00Z"))
        .unwrap();
    let coverage = service::coverage(&store, "CASE", None).unwrap();
    assert_eq!(coverage.earliest_known, Some(day("1970-01-01")));
    assert!(!coverage.enactment_known);
    assert_eq!(coverage.stored_revisions, 1);
    let blame = service::blame(
        &store,
        "CASE",
        "/Law/Sentence[index=1]",
        &query("1970-01-01"),
        day("1970-01-01"),
    )
    .unwrap();
    assert_eq!(blame.version, current);
    assert!(blame.previous_version.is_none());
    assert_eq!(blame.diagnostics[0].code, DiagnosticCode::CoverageGap);
    assert!(
        service::coverage(&store, "CASE", Some(instant("2026-01-01T00:00:00Z")))
            .unwrap()
            .enactment_known
    );
    let pinned = service::blame(
        &store,
        "CASE",
        "/Law/Sentence[index=1]",
        &Query {
            version: Some(original.clone()),
            ..query("1970-01-01")
        },
        day("1970-01-01"),
    )
    .unwrap();
    assert_eq!(pinned.version, original);
    assert!(pinned.previous_version.is_none());
    assert!(pinned.diagnostics.is_empty());
}

#[test]
fn blame_refuses_unordered_same_day_history_and_truncates_at_a_pinned_sequence() {
    let (_directory, store, mut base) = fixture();
    base.sequence = Some(1);
    base.tree = xml::parse_xml("<Law><Sentence>A</Sentence></Law>").unwrap();
    let first = store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    let mut later = base.clone();
    later.revision_id = "LATER".into();
    later.sequence = Some(2);
    later.tree = xml::parse_xml("<Law><Sentence>B</Sentence></Law>").unwrap();
    let second = store
        .save_revision(&later, instant("2026-01-02T00:00:00Z"))
        .unwrap();
    let path = "/Law/Sentence[index=1]";
    let pinned = service::blame(
        &store,
        "CASE",
        path,
        &Query {
            version: Some(first.clone()),
            ..query("1970-01-01")
        },
        day("1970-01-01"),
    )
    .unwrap();
    assert_eq!(pinned.version, first);
    assert!(pinned.previous_version.is_none());
    let current = service::blame(
        &store,
        "CASE",
        path,
        &query("1970-01-01"),
        day("1970-01-01"),
    )
    .unwrap();
    assert_eq!(current.version, second);
    assert_eq!(current.previous_version, Some(first));
    later.sequence = None;
    let ambiguous = store
        .save_revision(&later, instant("2026-01-03T00:00:00Z"))
        .unwrap();
    assert!(matches!(
        service::blame(
            &store,
            "CASE",
            path,
            &Query {
                version: Some(ambiguous),
                ..query("1970-01-01")
            },
            day("1970-01-01")
        ),
        Err(HistoryError::Ambiguous(_))
    ));
    later.revision_id = "UNKNOWN_ENFORCEMENT".into();
    later.effective = LegalDate::Uncertain {
        original: "Pending".into(),
        reason: "No established date".into(),
    };
    let unknown = store
        .save_revision(&later, instant("2026-01-04T00:00:00Z"))
        .unwrap();
    assert!(
        matches!(service::blame(&store, "CASE", path, &Query { version:Some(unknown), ..query("1970-01-01") }, day("1970-01-01")), Err(HistoryError::Ambiguous(items)) if items.iter().any(|item| item.code == DiagnosticCode::UnknownDate))
    );
}

#[test]
fn same_day_sequences_and_known_termination_dates_are_inclusive() {
    let (_directory, store, mut base) = fixture();
    base.sequence = Some(1);
    store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    base.revision_id = "LATER".into();
    base.sequence = Some(2);
    let later = store
        .save_revision(&base, instant("2026-01-02T00:00:00Z"))
        .unwrap();
    assert_eq!(
        service::select(&store, "CASE", &query("1970-01-01"), day("1970-01-01"))
            .unwrap()
            .selected
            .version,
        later
    );
    base.termination = Some(Termination {
        status: TerminationStatus::Expired,
        date: LegalDate::known(day("1970-01-02"), "Fixture termination"),
    });
    let terminated = store
        .save_revision(&base, instant("2026-01-03T00:00:00Z"))
        .unwrap();
    assert_eq!(
        service::select(&store, "CASE", &query("1970-01-01"), day("1970-01-03"))
            .unwrap()
            .selected
            .version,
        terminated
    );
    for at in ["1970-01-02", "1970-01-03"] {
        assert!(matches!(
            service::select(&store, "CASE", &query(at), day("1970-01-03")),
            Err(HistoryError::NotFound(_))
        ));
    }
}

#[test]
fn coverage_and_search_report_each_metadata_axis_without_conflating_text() {
    let (_directory, store, mut base) = fixture();
    base.law_id = "ID_ONLY".into();
    base.title = "META_ONLY".into();
    let raw = "<Law><Sentence>BODY_ONLY</Sentence></Law>";
    base.tree = xml::parse_xml(raw).unwrap();
    base.source.raw_sha256 = store.preserve(raw.as_bytes()).unwrap();
    let version = store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    let coverage = service::coverage(&store, "ID_ONLY", None).unwrap();
    assert_eq!(coverage.law_id, "ID_ONLY");
    assert_eq!(coverage.stored_revisions, 1);
    assert!(coverage.enactment_known);
    assert_eq!(coverage.earliest_known, Some(day("1970-01-01")));
    assert!(coverage.diagnostics.is_empty());
    for term in ["ID_ONLY", "META_ONLY", "BODY_ONLY"] {
        let result = service::search(&store, term, &Query::default(), day("1970-01-01")).unwrap();
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.hits[0].version, version);
    }
    assert!(
        service::search(&store, "absent", &Query::default(), day("1970-01-01"))
            .unwrap()
            .hits
            .is_empty()
    );
    base.law_id = "GAP".into();
    base.original_enactment = false;
    store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    assert_eq!(
        service::coverage(&store, "GAP", None).unwrap().diagnostics[0].code,
        DiagnosticCode::CoverageGap
    );
}

#[test]
fn reference_context_keeps_other_laws_when_inspecting_a_pinned_version() {
    let (_directory, store, mut base) = fixture();
    let raw = "<Law><MainProvision><Article Num=\"1\"><Sentence>第一条</Sentence></Article></MainProvision></Law>";
    base.tree = xml::parse_xml(raw).unwrap();
    base.source.raw_sha256 = store.preserve(raw.as_bytes()).unwrap();
    let original = store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    base.law_id = "OTHER".into();
    let other = store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    let pinned = Query {
        version: Some(original.clone()),
        ..query("1970-01-01")
    };
    let report = service::refs(&store, "CASE", &pinned, day("1970-01-01")).unwrap();
    assert_eq!(
        report.context_versions,
        BTreeMap::from([("CASE".into(), original), ("OTHER".into(), other)])
    );
    assert!(report.references.iter().all(|edge| {
        edge.from.law_id == "CASE"
            || edge
                .to
                .as_ref()
                .is_some_and(|target| target.law_id == "CASE")
    }));
    assert_eq!(report.references.len(), 1);
}

#[test]
fn scenario_bases_must_be_known_and_only_new_unresolved_references_are_broken() {
    use law_history::amendment::{AmendmentStep, Operation, Scenario, SourcedOperation};
    let (_directory, store, mut base) = fixture();
    let raw = "<Law><MainProvision><Article Num=\"1\"><Sentence>第九条及び第一条</Sentence></Article></MainProvision></Law>";
    base.tree = xml::parse_xml(raw).unwrap();
    base.source.raw_sha256 = store.preserve(raw.as_bytes()).unwrap();
    let original = store
        .save_revision(&base, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    let mut other = base.clone();
    other.law_id = "OTHER".into();
    let other_raw = "<Law><MainProvision><Article Num=\"1\"><Sentence>第十条及び第一条</Sentence></Article></MainProvision></Law>";
    other.tree = xml::parse_xml(other_raw).unwrap();
    other.source.raw_sha256 = store.preserve(other_raw.as_bytes()).unwrap();
    store
        .save_revision(&other, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    let path = "/Law/MainProvision[index=1]/Article[num=1]/Sentence[index=1]";
    let mut scenario = Scenario {
        schema_version: SCHEMA_VERSION,
        name: "Reference change".into(),
        bases: BTreeMap::from([("CASE".into(), original.clone())]),
        at: day("1970-01-01"),
        known_at: Some(instant("2026-01-01T00:00:00Z")),
        reference_context: None,
        steps: vec![AmendmentStep {
            id: "edit".into(),
            effective: LegalDate::known(day("1970-01-01"), "Fixture date"),
            after: Vec::new(),
            adjustments: Vec::new(),
            operations: vec![SourcedOperation {
                evidence: base.source.clone(),
                operation: Operation::ReplaceText {
                    law_id: "CASE".into(),
                    path: path.into(),
                    old: "第九条".into(),
                    new: "第十条".into(),
                },
            }],
        }],
    };
    let result = service::simulate(&store, &scenario).unwrap();
    assert!(result.diagnostics.is_empty());
    assert_eq!(result.impact.broken.len(), 1);
    assert_eq!(result.impact.broken[0].text, "第十条");
    assert_eq!(result.impact.broken[0].from.law_id, "CASE");
    assert!(result.impact.broken[0].to.is_none());
    assert!(
        result
            .impact
            .direct
            .iter()
            .all(|position| position.path != "/Law")
    );
    let blame = service::blame(
        &store,
        "CASE",
        path,
        &Query {
            version: Some(original.clone()),
            ..Query::default()
        },
        day("1970-01-01"),
    )
    .unwrap();
    assert!(blame.diagnostics.is_empty());
    let mut unchanged = scenario.clone();
    unchanged.steps.clear();
    assert!(
        service::simulate(&store, &unchanged)
            .unwrap()
            .impact
            .broken
            .is_empty()
    );
    let mut future = base.clone();
    future.revision_id = "FUTURE_KNOWLEDGE".into();
    let future = store
        .save_revision(&future, instant("2026-01-02T00:00:00Z"))
        .unwrap();
    scenario.bases.insert("CASE".into(), future);
    assert!(matches!(
        service::simulate(&store, &scenario),
        Err(HistoryError::NotFound(_))
    ));
}

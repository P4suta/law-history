use chrono::{DateTime, Utc};
use law_history::{
    Store,
    amendment::{self, AmendmentStep, Operation, Scenario, SourcedOperation},
    ingest,
    model::*,
    references, service, xml,
};
use std::collections::BTreeMap;

fn instant(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}
fn day(value: &str) -> Day {
    value.parse().unwrap()
}
fn demo() -> (tempfile::TempDir, Store, Vec<StoredRevision>) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    ingest::import_bundle(&store, include_bytes!("../../../fixtures/demo.json")).unwrap();
    let history = store.history("DEMO_ACT", None).unwrap();
    (directory, store, history)
}

fn scenario(history: &[StoredRevision]) -> Scenario {
    Scenario {
        schema_version: SCHEMA_VERSION,
        name: "Electronic submission".into(),
        bases: BTreeMap::from([("DEMO_ACT".into(), history[0].version.clone())]),
        at: day("1970-04-01"),
        known_at: None,
        reference_context: None,
        steps: vec![AmendmentStep {
            id: "electronic-submission".into(),
            effective: LegalDate::known(day("1970-04-01"), "Synthetic fixture"),
            after: vec![],
            adjustments: vec![],
            operations: vec![
                amendment::parse_amending_sentence(
                    "第一条中「書面」を「電磁的記録」に改める。",
                    "DEMO_ACT",
                    &history[0].revision.tree,
                    history[0].revision.source.clone(),
                )
                .unwrap(),
            ],
        }],
    }
}

#[test]
fn calendar_dates_survive_serialization_without_unix_time() {
    for value in [
        "0001-01-01",
        "1868-01-01",
        "1969-12-31",
        "1970-01-01",
        "2000-02-29",
        "9999-12-31",
    ] {
        let parsed = day(value);
        assert_eq!(
            serde_json::from_str::<Day>(&serde_json::to_string(&parsed).unwrap()).unwrap(),
            parsed
        );
    }
    for value in [
        "1900-02-29",
        "1970-00-01",
        "2026-04-31",
        "0000-01-01",
        "２０２６-01-01",
        "2026-1-01",
    ] {
        assert!(value.parse::<Day>().is_err());
    }
}

#[test]
fn numeral_tables_preserve_digits_units_and_invalid_sequences() {
    for (text, expected) in [
        ("〇", 0),
        ("零", 0),
        ("一", 1),
        ("二", 2),
        ("三", 3),
        ("四", 4),
        ("五", 5),
        ("六", 6),
        ("七", 7),
        ("八", 8),
        ("九", 9),
        ("十", 10),
        ("百", 100),
        ("千", 1000),
        ("九千九百九十九", 9999),
    ] {
        assert_eq!(references::japanese_number(text), Some(expected));
    }
    for invalid in ["", "一二", "十百", "百百", "一万", "4294967296"] {
        assert_eq!(references::japanese_number(invalid), None);
    }
}

#[test]
fn preserved_fuzz_seeds_replay_calendar_and_amendment_boundaries() {
    let source = include_bytes!("../../../fuzz/corpus/object_codec/text.txt");
    let encoded = law_history::codec::encode(source).unwrap();
    assert_eq!(
        law_history::codec::decode(&encoded, source.len()).unwrap(),
        source
    );
    let parsed: Day = serde_json::from_str(include_str!(
        "../../../fuzz/corpus/legal_day/pre_epoch.json"
    ))
    .unwrap();
    assert_eq!(parsed, day("1969-12-31"));
    assert!(
        serde_json::from_str::<Day>(include_str!(
            "../../../fuzz/corpus/legal_day/invalid_leap.json"
        ))
        .is_err()
    );
    let uncertain: LegalDate = serde_json::from_str(include_str!(
        "../../../fuzz/corpus/legal_day/uncertain.json"
    ))
    .unwrap();
    assert_eq!(uncertain.day(), None);
    let (_directory, _store, history) = demo();
    for (input, accepted) in [
        (
            include_str!("../../../fuzz/corpus/amendment_text/replace.txt"),
            true,
        ),
        (
            include_str!("../../../fuzz/corpus/amendment_text/delete.txt"),
            true,
        ),
        (
            include_str!("../../../fuzz/corpus/amendment_text/unknown.txt"),
            false,
        ),
    ] {
        let parsed = amendment::parse_document(
            input,
            "DEMO_ACT",
            &history[0].revision.tree,
            &history[0].revision.source,
        );
        assert_eq!(parsed.diagnostics.is_empty(), accepted);
        assert_eq!(!parsed.operations.is_empty(), accepted);
    }
    let tree = xml::parse_xml(include_str!("../../../fuzz/corpus/law_xml/provisions.xml")).unwrap();
    assert!(
        xml::flatten(&tree)
            .keys()
            .any(|path| path.contains("/Article[num=1]"))
    );
}

#[test]
fn enforcement_and_observation_are_independent() {
    let (_directory, store, history) = demo();
    assert_eq!(history.len(), 2);
    let query = Query {
        at: Some(day("1970-04-01")),
        known_at: Some(instant("2026-01-01T12:00:00Z")),
        version: None,
    };
    assert_eq!(
        service::select(&store, "DEMO_ACT", &query, day("2026-01-01"))
            .unwrap()
            .selected
            .version,
        history[0].version
    );
    let query = Query {
        at: Some(day("1970-04-01")),
        ..Default::default()
    };
    assert_eq!(
        service::select(&store, "DEMO_ACT", &query, day("2026-01-01"))
            .unwrap()
            .selected
            .version,
        history[1].version
    );
    assert_eq!(
        service::select(&store, "DEMO_ACT", &Query::default(), day("1969-12-31"))
            .unwrap()
            .selected
            .version,
        history[0].version
    );
    assert!(service::select(&store, "DEMO_ACT", &Query::default(), day("1969-01-01")).is_err());
}

#[test]
fn reimport_recovery_and_reindex_preserve_selection_and_ids() {
    let (directory, store, history) = demo();
    ingest::import_bundle(&store, include_bytes!("../../../fixtures/demo.json")).unwrap();
    assert_eq!(store.history("DEMO_ACT", None).unwrap().len(), 2);
    let expected = serde_json::to_vec(
        &service::select(&store, "DEMO_ACT", &Query::default(), day("2026-01-01")).unwrap(),
    )
    .unwrap();
    assert!(store.rebuild().unwrap() > 0);
    assert_eq!(
        expected,
        serde_json::to_vec(
            &service::select(&store, "DEMO_ACT", &Query::default(), day("2026-01-01")).unwrap()
        )
        .unwrap()
    );
    drop(store);
    std::fs::remove_file(directory.path().join("index.sqlite3")).unwrap();
    let rebuilt = Store::open(directory.path()).unwrap();
    rebuilt.rebuild().unwrap();
    assert_eq!(
        history[1].version,
        service::select(&rebuilt, "DEMO_ACT", &Query::default(), day("2026-01-01"))
            .unwrap()
            .selected
            .version
    );
    assert!(rebuilt.verify_integrity().unwrap() >= 6);
}

#[test]
fn amendments_reproduce_expected_tree_and_fail_atomically() {
    let (_directory, store, history) = demo();
    let mut scenario = scenario(&history);
    let result = service::simulate(&store, &scenario).unwrap();
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.results["DEMO_ACT"].tree, history[1].revision.tree);
    scenario.steps[0].operations.push(SourcedOperation {
        operation: Operation::ReplaceText {
            law_id: "MISSING_LAW".into(),
            path: "/Law".into(),
            old: "x".into(),
            new: "y".into(),
        },
        evidence: history[0].revision.source.clone(),
    });
    let refused = service::simulate(&store, &scenario).unwrap();
    assert!(refused.results.is_empty());
    assert!(!refused.diagnostics.is_empty());
    assert_eq!(
        store.version(&history[0].version).unwrap().tree,
        history[0].revision.tree
    );
}

#[test]
fn staged_same_day_and_unknown_enforcement_are_explicit() {
    let (_directory, store, history) = demo();
    let mut scenario = scenario(&history);
    let mut second = scenario.steps[0].clone();
    second.id = "follow-up".into();
    second.operations = vec![SourcedOperation {
        operation: Operation::ReplaceText {
            law_id: "DEMO_ACT".into(),
            path: scenario.steps[0].operations[0].operation.path().into(),
            old: "電磁的記録".into(),
            new: "記録".into(),
        },
        evidence: history[0].revision.source.clone(),
    }];
    scenario.steps.push(second);
    assert_eq!(
        service::simulate(&store, &scenario).unwrap().diagnostics[0].code,
        DiagnosticCode::AmbiguousOrder
    );
    scenario.steps[1].after.push("electronic-submission".into());
    assert!(
        service::simulate(&store, &scenario)
            .unwrap()
            .diagnostics
            .is_empty()
    );
    scenario.steps[1].effective = LegalDate::Uncertain {
        original: "政令で定める日".into(),
        reason: "No enforcing instrument".into(),
    };
    assert_eq!(
        service::simulate(&store, &scenario).unwrap().diagnostics[0].code,
        DiagnosticCode::UnknownDate
    );
    scenario.steps.pop();
    scenario.at = day("1970-03-31");
    assert_eq!(
        service::simulate(&store, &scenario).unwrap().results["DEMO_ACT"].tree,
        history[0].revision.tree
    );
}

#[test]
fn same_day_official_revisions_require_order() {
    let (_directory, store, history) = demo();
    let mut collision = history[1].revision.clone();
    collision.revision_id = "DEMO_ACT_SAME_DAY".into();
    collision.sequence = None;
    store
        .save_revision(&collision, instant("2026-01-03T00:00:00Z"))
        .unwrap();
    assert!(matches!(
        service::select(&store, "DEMO_ACT", &Query::default(), day("2026-01-01")),
        Err(HistoryError::Ambiguous(_))
    ));
}

#[test]
fn correction_knowledge_does_not_rewrite_earlier_results() {
    let (_directory, store, history) = demo();
    let mut correction = history[1].revision.clone();
    correction.kind = RevisionKind::Correction;
    correction.tree = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1\"><Paragraph Num=\"1\"><Sentence>訂正後</Sentence></Paragraph></Article></MainProvision></LawBody></Law>").unwrap();
    let changed = store
        .save_revision(&correction, instant("2026-01-03T00:00:00Z"))
        .unwrap();
    assert_eq!(
        service::select(&store, "DEMO_ACT", &Query::default(), day("2026-01-01"))
            .unwrap()
            .selected
            .version,
        changed
    );
    assert_eq!(
        service::select(
            &store,
            "DEMO_ACT",
            &Query {
                known_at: Some(instant("2026-01-02T12:00:00Z")),
                ..Default::default()
            },
            day("2026-01-01")
        )
        .unwrap()
        .selected
        .version,
        history[1].version
    );
}

#[test]
fn references_include_cycles_and_separate_review_from_direct_change() {
    let (_directory, store, history) = demo();
    let refs = service::refs(&store, "DEMO_ACT", &Query::default(), day("2026-01-01")).unwrap();
    assert!(
        refs.references
            .iter()
            .any(|edge| edge.kind == references::ReferenceKind::Delegation && edge.to.is_none())
    );
    assert_eq!(
        refs.references
            .iter()
            .filter(|edge| edge.to.is_some())
            .count(),
        3
    );
    let impact = service::impact(
        &store,
        &history[0].version,
        &history[1].version,
        &Query::default(),
        day("2026-01-01"),
    )
    .unwrap();
    assert!(!impact.direct.is_empty());
    assert_eq!(impact.review.len(), 2);
    assert!(impact.review.iter().all(|item| !item.via.is_empty()));
}

#[test]
fn lossless_structure_includes_mixed_text_appendices_and_items() {
    let tree = xml::parse_xml("<Law><Sentence>甲<Ruby>乙<Rt>おつ</Rt></Ruby>丙</Sentence><SupplProvision/><AppdxTable Num=\"1\"><Item Num=\"1\"/></AppdxTable></Law>").unwrap();
    assert_eq!(tree.text(), "甲乙おつ丙");
    assert_eq!(
        xml::parse_xml("<Law><Sentence><Ruby>甲</Ruby> <Ruby>乙</Ruby></Sentence></Law>")
            .unwrap()
            .text(),
        "甲 乙"
    );
    let encoded = serde_json::to_vec(&tree).unwrap();
    assert_eq!(serde_json::from_slice::<Element>(&encoded).unwrap(), tree);
    assert!(xml::flatten(&tree).contains_key("/Law/AppdxTable[num=1]/Item[num=1]"));
    assert!(xml::parse_xml("<!DOCTYPE Law [<!ENTITY x 'bad'>]><Law>&x;</Law>").is_err());
    assert!(xml::parse_xml("<Law xmlns=\"urn:other\"><Article/></Law>").is_err());
    assert!(xml::parse_xml("<Law xmlns:x=\"urn:other\" x:Num=\"1\"/>").is_err());
}

#[test]
fn zero_and_empty_appendix_numbers_use_distinct_position_addresses() {
    let tree = xml::parse_xml(include_str!("../../../fixtures/numberless-appendices.xml")).unwrap();
    let nodes = xml::index(&tree);
    for ordinal in 1..=3 {
        assert!(nodes.contains_key(&format!(
            "/Law/LawBody[index=1]/AppdxTable[index={ordinal}]"
        )));
    }
    assert_eq!(
        nodes["/Law/LawBody[index=1]/AppdxTable[index=1]"].attributes["Num"],
        "0"
    );
    let path = "/Law/LawBody[index=1]/AppdxTable[index=2]";
    let changed = xml::replace(&tree, path, None).unwrap();
    assert!(!changed.text().contains("第一号表ノ二"));
    assert!(changed.text().contains("（別表）第一号表"));
}

#[test]
fn compaction_preserves_content_addresses_legacy_reads_and_index_recovery() {
    let (directory, store, history) = demo();
    let bytes = b"Legacy source text repeated for compression. ".repeat(1000);
    let digest = law_history::store::sha256(&bytes);
    let raw = directory.path().join("sources").join(&digest);
    std::fs::write(&raw, &bytes).unwrap();
    assert_eq!(store.source(&digest).unwrap(), bytes);
    let preview = store.compact(true).unwrap();
    assert!(raw.exists());
    assert!(preview.object_bytes_after < preview.object_bytes_before);
    let report = store.compact(false).unwrap();
    assert!(!raw.exists());
    assert_eq!(store.source(&digest).unwrap(), bytes);
    assert_eq!(report.object_bytes_after, preview.object_bytes_after);
    assert_eq!(store.preserve(&bytes).unwrap(), digest);
    store.rebuild().unwrap();
    assert_eq!(
        service::select(&store, "DEMO_ACT", &Query::default(), day("2026-11-05"))
            .unwrap()
            .selected
            .version,
        history[1].version
    );
    let encoded = directory
        .path()
        .join("sources")
        .join(format!("{digest}.zst"));
    std::fs::write(encoded, b"corrupt compressed frame").unwrap();
    assert!(store.source(&digest).is_err());
    assert!(store.verify_integrity().is_err());
}

#[test]
fn object_decoding_enforces_output_limits() {
    let encoded = law_history::codec::encode(&vec![b'x'; 4096]).unwrap();
    assert!(law_history::codec::decode(&encoded, 4095).is_err());
    assert_eq!(
        law_history::codec::decode(&encoded, 4096).unwrap(),
        vec![b'x'; 4096]
    );
    assert!(law_history::codec::decode(b"invalid frame", 4096).is_err());
}

#[test]
fn wrong_original_and_unsupported_syntax_produce_source_locations() {
    let (_directory, _store, history) = demo();
    let evidence = history[0].revision.source.clone();
    let error = amendment::parse_amending_sentence(
        "不明な改正文",
        "DEMO_ACT",
        &history[0].revision.tree,
        evidence.clone(),
    )
    .unwrap_err();
    assert_eq!(error.code, DiagnosticCode::UnsupportedSyntax);
    assert_eq!(error.source, evidence.url);
    let operation = SourcedOperation {
        operation: Operation::ReplaceText {
            law_id: "DEMO_ACT".into(),
            path: "/Law".into(),
            old: "不存在".into(),
            new: "追加".into(),
        },
        evidence,
    };
    assert_eq!(
        amendment::apply_operation(&history[0].revision.tree, &operation)
            .unwrap_err()
            .code,
        DiagnosticCode::OriginalMismatch
    );
}

#[test]
fn overlapping_text_matches_are_ambiguous() {
    let (_directory, _store, history) = demo();
    let tree = xml::parse_xml("<Law><Sentence>甲甲甲</Sentence></Law>").unwrap();
    let original = tree.clone();
    let operation = SourcedOperation {
        operation: Operation::ReplaceText {
            law_id: "DEMO_ACT".into(),
            path: "/Law".into(),
            old: "甲甲".into(),
            new: "乙".into(),
        },
        evidence: history[0].revision.source.clone(),
    };
    assert_eq!(
        amendment::apply_operation(&tree, &operation)
            .unwrap_err()
            .code,
        DiagnosticCode::OriginalMismatch
    );
    assert_eq!(tree, original);
}

#[test]
fn immutable_corruption_fails_rebuild_without_destroying_index() {
    let (directory, store, history) = demo();
    std::fs::write(
        directory.path().join("versions").join(&history[0].version),
        b"corrupt",
    )
    .unwrap();
    assert!(store.rebuild().is_err());
    assert!(store.law_ids().unwrap().contains(&"DEMO_ACT".to_string()));
    assert!(store.history("DEMO_ACT", None).is_err());
    assert!(store.verify_integrity().is_err());
}

#[test]
fn selected_index_metadata_must_match_the_preserved_version() {
    let (directory, store, history) = demo();
    let db = rusqlite::Connection::open(directory.path().join("index.sqlite3")).unwrap();
    db.execute(
        "UPDATE revisions SET json=json_set(json,'$.effective.date','1960-01-01')",
        [],
    )
    .unwrap();
    assert!(matches!(
        service::select(
            &store,
            "DEMO_ACT",
            &Query {
                version: Some(history[0].version.clone()),
                ..Default::default()
            },
            day("2026-11-05")
        ),
        Err(HistoryError::Invalid(_))
    ));
    store.rebuild().unwrap();
    assert!(service::select(&store, "DEMO_ACT", &Query::default(), day("2026-11-05")).is_ok());
}

#[test]
fn saved_scenarios_pin_reference_versions_diagnostics_and_absent_laws() {
    let (_directory, store, history) = demo();
    let mut other = history[0].revision.clone();
    other.law_id = "CONTEXT".into();
    other.revision_id = "CONTEXT_BASE".into();
    other.title = "Context law".into();
    let context_version = store
        .save_revision(&other, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    let mut pending = other.clone();
    pending.revision_id = "CONTEXT_PENDING".into();
    pending.effective = LegalDate::Uncertain {
        original: "Pending date".into(),
        reason: "Synthetic uncertainty".into(),
    };
    store
        .save_revision(&pending, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    let preview = scenario(&history);
    let id = service::save_scenario(&store, &preview, instant("2026-02-01T00:00:00Z")).unwrap();
    let saved: Scenario = store
        .latest_fact(&format!("scenario:{id}"), None)
        .unwrap()
        .unwrap();
    let context = saved.reference_context.as_ref().unwrap();
    assert_eq!(context.versions["CONTEXT"], context_version);
    assert!(!context.versions.contains_key("DEMO_ACT"));
    assert!(
        context
            .diagnostics
            .iter()
            .any(|item| item.code == DiagnosticCode::UnknownDate)
    );
    let before = serde_json::to_vec(&service::simulate(&store, &saved).unwrap()).unwrap();
    other.title = "Late backdated correction".into();
    let corrected = store
        .save_revision(&other, instant("2026-01-03T00:00:00Z"))
        .unwrap();
    other.law_id = "LATE_CONTEXT".into();
    store
        .save_revision(&other, instant("2026-01-03T00:00:00Z"))
        .unwrap();
    assert_eq!(
        serde_json::to_vec(&service::simulate(&store, &saved).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        service::save_scenario(&store, &saved, instant("2026-03-01T00:00:00Z")).unwrap(),
        id
    );
    let dynamic = service::simulate(&store, &preview).unwrap();
    assert_eq!(dynamic.impact.context_versions["CONTEXT"], corrected);
    assert!(dynamic.impact.context_versions.contains_key("LATE_CONTEXT"));
    for (law, version) in [
        ("DEMO_ACT", history[0].version.clone()),
        ("WRONG_LAW", context_version),
        ("CONTEXT", "a".repeat(64)),
    ] {
        let mut invalid = saved.clone();
        invalid
            .reference_context
            .as_mut()
            .unwrap()
            .versions
            .insert(law.into(), version);
        assert!(service::simulate(&store, &invalid).is_err());
    }
    store.rebuild().unwrap();
    assert_eq!(
        serde_json::to_vec(&service::simulate(&store, &saved).unwrap()).unwrap(),
        before
    );
}

#[test]
fn scenario_only_persists_on_explicit_save() {
    let (_directory, store, history) = demo();
    let scenario = scenario(&history);
    let simulation = service::simulate(&store, &scenario).unwrap();
    assert!(
        store
            .latest_fact::<Scenario>(&format!("scenario:{}", simulation.scenario_id), None)
            .unwrap()
            .is_none()
    );
    let id = service::save_scenario(&store, &scenario, instant("2026-02-01T00:00:00Z")).unwrap();
    let saved: Scenario = store
        .latest_fact(&format!("scenario:{id}"), None)
        .unwrap()
        .unwrap();
    assert_eq!(saved.known_at, Some(instant("2026-02-01T00:00:00Z")));
    assert_eq!(
        id,
        law_history::store::sha256(&serde_json::to_vec(&saved).unwrap())
    );
    assert!(
        store
            .latest_fact::<Scenario>(&format!("scenario:{id}"), None)
            .unwrap()
            .is_some()
    );
}

#[test]
fn japan_calendar_day_changes_at_fifteen_utc() {
    assert_eq!(
        japan_day(instant("2026-11-04T14:59:59Z")).unwrap(),
        day("2026-11-04")
    );
    assert_eq!(
        japan_day(instant("2026-11-04T15:00:00Z")).unwrap(),
        day("2026-11-05")
    );
}

#[test]
fn returning_to_previous_text_uses_the_latest_observation() {
    let (_directory, store, history) = demo();
    let mut correction = history[1].revision.clone();
    correction.title = "Corrected title".into();
    let corrected = store
        .save_revision(&correction, instant("2026-01-03T00:00:00Z"))
        .unwrap();
    store
        .save_revision(&history[1].revision, instant("2026-01-04T00:00:00Z"))
        .unwrap();
    assert_eq!(
        service::select(&store, "DEMO_ACT", &Query::default(), day("2026-11-05"))
            .unwrap()
            .selected
            .version,
        history[1].version
    );
    assert_eq!(
        service::select(
            &store,
            "DEMO_ACT",
            &Query {
                known_at: Some(instant("2026-01-03T12:00:00Z")),
                ..Default::default()
            },
            day("2026-11-05")
        )
        .unwrap()
        .selected
        .version,
        corrected
    );
}

#[test]
fn unknown_termination_processing_dates_do_not_imply_active_law() {
    let (_directory, store, history) = demo();
    let mut expired = history[1].revision.clone();
    expired.termination = Some(Termination {
        status: TerminationStatus::Expired,
        date: LegalDate::Uncertain {
            original: "Processing date 2026-01-03".into(),
            reason: "No legal termination date".into(),
        },
    });
    let version = store
        .save_revision(&expired, instant("2026-01-03T00:00:00Z"))
        .unwrap();
    assert!(matches!(
        service::select(&store, "DEMO_ACT", &Query::default(), day("2026-11-05")),
        Err(HistoryError::Ambiguous(_))
    ));
    assert!(
        service::select(
            &store,
            "DEMO_ACT",
            &Query {
                version: Some(version),
                ..Default::default()
            },
            day("2026-11-05")
        )
        .is_ok()
    );
    let search = service::search(
        &store,
        "Demonstration",
        &Query::default(),
        day("2026-11-05"),
    )
    .unwrap();
    assert!(search.hits.is_empty());
    assert!(!search.diagnostics.is_empty());
}

#[test]
fn structured_amendments_replace_and_insert_articles_without_guessing() {
    let (_directory, _store, history) = demo();
    let evidence = &history[0].revision.source;
    let input = "<Law><AmendProvision><AmendProvisionSentence><Sentence>第一条を次のように改める。</Sentence></AmendProvisionSentence><NewProvision><Article Num=\"1\"><ArticleTitle>第一条</ArticleTitle><Paragraph Num=\"1\"><Sentence>置換した本文。</Sentence></Paragraph></Article></NewProvision></AmendProvision></Law>";
    let parsed = amendment::parse_document(input, "DEMO_ACT", &history[0].revision.tree, evidence);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let applied =
        amendment::apply_operation(&history[0].revision.tree, &parsed.operations[0]).unwrap();
    assert!(applied.text().contains("置換した本文。"));
    let mixed = amendment::parse_document(
        "第一条中「書面」を「記録」に改める。\n解釈できない改正文",
        "DEMO_ACT",
        &history[0].revision.tree,
        evidence,
    );
    assert!(mixed.operations.is_empty());
    assert_eq!(mixed.diagnostics.len(), 1);
    assert_eq!(mixed.diagnostics[0].locator, "line:2");
}

#[test]
fn inserted_provisions_appear_in_direct_impact_and_references_are_deduplicated() {
    let (_directory, store, history) = demo();
    let tree = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1\"><Paragraph Num=\"1\"><Sentence>第一条及び第一条。</Sentence></Paragraph></Article></MainProvision></LawBody></Law>").unwrap();
    let mut revision = history[0].revision.clone();
    revision.tree = tree;
    let corpus = BTreeMap::from([("DEMO_ACT".into(), (history[0].version.clone(), revision))]);
    assert_eq!(references::analyze(&corpus).len(), 1);
    let mut proposed = scenario(&history);
    proposed.steps[0].operations = vec![SourcedOperation {
        operation: Operation::Insert {
            law_id: "DEMO_ACT".into(),
            parent: "/Law/LawBody[index=1]/MainProvision[index=1]".into(),
            before: None,
            element: xml::parse_xml("<Article Num=\"4\"><ArticleTitle>第四条</ArticleTitle><Paragraph Num=\"1\"><Sentence>追加した規定。</Sentence></Paragraph></Article>").unwrap(),
        },
        evidence: history[0].revision.source.clone(),
    }];
    let simulation = service::simulate(&store, &proposed).unwrap();
    assert!(
        simulation.diagnostics.is_empty(),
        "{:?}",
        simulation.diagnostics
    );
    assert!(simulation.impact.direct.iter().any(|position| {
        position.path.ends_with("/Article[num=4]") && position.version == simulation.scenario_id
    }));
}

#[test]
fn renumbering_updates_the_title_and_rejects_collisions() {
    let (_directory, _store, history) = demo();
    let parsed = amendment::parse_document(
        "第一条を第四条とする。",
        "DEMO_ACT",
        &history[0].revision.tree,
        &history[0].revision.source,
    );
    assert!(parsed.diagnostics.is_empty());
    let changed =
        amendment::apply_operation(&history[0].revision.tree, &parsed.operations[0]).unwrap();
    let index = xml::flatten(&changed);
    assert!(index.keys().any(|path| path.ends_with("/Article[num=4]")));
    assert!(changed.text().contains("第四条"));
    let collision = amendment::parse_document(
        "第一条を第二条とする。",
        "DEMO_ACT",
        &history[0].revision.tree,
        &history[0].revision.source,
    );
    assert_eq!(
        amendment::apply_operation(&history[0].revision.tree, &collision.operations[0])
            .unwrap_err()
            .code,
        DiagnosticCode::Conflict
    );
    let repeated = xml::parse_xml("<Law><Article Num=\"1\"/><Article Num=\"1\"/></Law>").unwrap();
    assert!(xml::index(&repeated).contains_key("/Law/Article[index=1]"));
    assert!(xml::index(&repeated).contains_key("/Law/Article[index=2]"));
}

#[test]
fn adjustments_change_an_unenforced_step_and_preserve_its_schedule() {
    let (_directory, store, history) = demo();
    let mut scenario = scenario(&history);
    scenario.steps[0].effective =
        LegalDate::known(day("1971-04-01"), "Synthetic delayed enforcement");
    scenario.steps[0].after.push("adjust-pending".into());
    let original_hash =
        law_history::store::sha256(&serde_json::to_vec(&scenario.steps[0].operations).unwrap());
    let replacement = SourcedOperation {
        operation: Operation::ReplaceText {
            law_id: "DEMO_ACT".into(),
            path: scenario.steps[0].operations[0].operation.path().into(),
            old: "書面".into(),
            new: "記録".into(),
        },
        evidence: history[0].revision.source.clone(),
    };
    scenario.steps.push(AmendmentStep {
        id: "adjust-pending".into(),
        effective: LegalDate::known(day("1970-04-01"), "Synthetic adjustment"),
        after: vec![],
        operations: vec![],
        adjustments: vec![amendment::Adjustment {
            target_step: "electronic-submission".into(),
            expected_operations_sha256: original_hash,
            replacement: vec![replacement],
        }],
    });
    let pending = service::simulate(&store, &scenario).unwrap();
    assert!(pending.diagnostics.is_empty(), "{:?}", pending.diagnostics);
    assert_eq!(pending.results["DEMO_ACT"].tree, history[0].revision.tree);
    scenario.at = day("1971-04-01");
    let applied = service::simulate(&store, &scenario).unwrap();
    assert!(applied.diagnostics.is_empty());
    assert!(
        applied.results["DEMO_ACT"]
            .tree
            .text()
            .contains("申請は記録で行う。")
    );
    assert_eq!(
        applied.results["DEMO_ACT"].source.confidence,
        Confidence::Inferred
    );
}

#[test]
fn multi_law_batch_recovery_rejects_any_invalid_member() {
    let (_directory, store, history) = demo();
    let mut first = history[0].revision.clone();
    first.law_id = "BATCH_FIRST".into();
    let mut second = first.clone();
    second.law_id = "BATCH_SECOND".into();
    second.source.raw_sha256 = "0".repeat(64);
    assert!(
        store
            .save_revisions(
                &[first.clone(), second.clone()],
                instant("2026-02-01T00:00:00Z")
            )
            .is_err()
    );
    assert!(store.history("BATCH_FIRST", None).unwrap().is_empty());
    second.source = first.source.clone();
    store
        .save_revisions(&[first, second], instant("2026-02-01T00:00:00Z"))
        .unwrap();
    store.rebuild().unwrap();
    assert_eq!(store.history("BATCH_FIRST", None).unwrap().len(), 1);
    assert_eq!(store.history("BATCH_SECOND", None).unwrap().len(), 1);
}

#[test]
fn comparison_records_and_reconstruction_preserve_their_evidence_boundary() {
    use law_history::reconstruction::{self, OfficialComparison, Reconstruction};
    let (_directory, store, history) = demo();
    let proposed = scenario(&history);
    let mut official_fixture = history[1].revision.clone();
    official_fixture.revision_id = "DEMO_ACT_OFFICIAL_BOUNDARY_FIXTURE".into();
    official_fixture.source.confidence = Confidence::Official;
    let target = store
        .save_revision(&official_fixture, instant("2026-02-01T00:00:00Z"))
        .unwrap();
    let comparison = OfficialComparison {
        schema_version: SCHEMA_VERSION,
        scenario: proposed.clone(),
        expected_versions: BTreeMap::from([("DEMO_ACT".into(), target.clone())]),
    };
    let matched = reconstruction::record_comparison(
        &store,
        &serde_json::to_vec(&comparison).unwrap(),
        instant("2026-02-02T00:00:00Z"),
    )
    .unwrap();
    assert_eq!(matched.matched_versions, vec![target]);
    assert!(matched.diagnostics.is_empty());
    let mut circular = comparison.clone();
    circular.expected_versions = proposed.bases.clone();
    assert!(reconstruction::compare(&store, &circular).is_err());
    official_fixture.tree =
        xml::parse_xml("<Law><Sentence>Different independently supplied text</Sentence></Law>")
            .unwrap();
    let mismatch = store
        .save_revision(&official_fixture, instant("2026-02-03T00:00:00Z"))
        .unwrap();
    let mut wrong = comparison;
    wrong.expected_versions.insert("DEMO_ACT".into(), mismatch);
    let refused = reconstruction::record_comparison(
        &store,
        &serde_json::to_vec(&wrong).unwrap(),
        instant("2026-02-04T00:00:00Z"),
    )
    .unwrap();
    assert_eq!(
        refused.diagnostics[0].code,
        DiagnosticCode::OfficialMismatch
    );
    assert_eq!(
        service::status(&store, None).unwrap().diagnostics[0].code,
        DiagnosticCode::OfficialMismatch
    );
    let reconstruction = Reconstruction {
        schema_version: SCHEMA_VERSION,
        scenario: proposed,
        revision_ids: BTreeMap::from([("DEMO_ACT".into(), "DEMO_ACT_RECONSTRUCTED".into())]),
        evidence: history[0].revision.source.clone(),
    };
    let versions = reconstruction::reconstruct(
        &store,
        &serde_json::to_vec(&reconstruction).unwrap(),
        instant("2026-02-05T00:00:00Z"),
    )
    .unwrap();
    let restored = store.version(&versions[0]).unwrap();
    assert_eq!(restored.tree, history[1].revision.tree);
    assert_eq!(restored.source.confidence, Confidence::Reconstructed);
    assert!(!restored.original_enactment);
    store.rebuild().unwrap();
    assert_eq!(store.version(&versions[0]).unwrap(), restored);
}

#[test]
fn moved_appendices_record_paths_and_repeal_breaks_incoming_references() {
    let (_directory, store, history) = demo();
    let mut proposed = scenario(&history);
    let path = "/Law/LawBody[index=1]/AppdxTable[num=1]";
    let element = xml::index(&history[0].revision.tree)[path].clone();
    proposed.steps[0].operations = vec![SourcedOperation {
        operation: Operation::Move {
            law_id: "DEMO_ACT".into(),
            path: path.into(),
            expected: element,
            parent: "/Law/LawBody[index=1]/SupplProvision[index=1]".into(),
            before: None,
        },
        evidence: history[0].revision.source.clone(),
    }];
    let moved = service::simulate(&store, &proposed).unwrap();
    assert!(moved.diagnostics.is_empty(), "{:?}", moved.diagnostics);
    let relation = moved.diffs[0]
        .changes
        .iter()
        .find(|change| matches!(change.kind, ChangeKind::Moved))
        .unwrap();
    assert_eq!(relation.before.as_ref().unwrap().path, path);
    assert!(
        relation
            .after
            .as_ref()
            .unwrap()
            .path
            .contains("/SupplProvision[index=1]/AppdxTable")
    );
    assert_eq!(
        relation.evidence.as_ref().unwrap(),
        &history[0].revision.source
    );
    let mut referencing = history[0].revision.clone();
    referencing.law_id = "RELATED_ACT".into();
    referencing.revision_id = "RELATED_ACT_ENACTMENT".into();
    referencing.title = "Related Act".into();
    referencing.tree = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1\"><Paragraph Num=\"1\"><Sentence>Demonstration Act第一条。</Sentence></Paragraph></Article></MainProvision></LawBody></Law>").unwrap();
    store
        .save_revision(&referencing, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    proposed.steps[0].operations[0].operation = Operation::Repeal {
        law_id: "DEMO_ACT".into(),
    };
    let repealed = service::simulate(&store, &proposed).unwrap();
    assert!(repealed.diagnostics.is_empty());
    assert_eq!(repealed.results["DEMO_ACT"].repeal, Some(day("1970-04-01")));
    assert!(
        repealed
            .impact
            .broken
            .iter()
            .any(|edge| edge.from.law_id == "RELATED_ACT")
    );
}

#[test]
fn official_comparison_checks_law_identity_confidence_and_all_result_states() {
    use law_history::reconstruction::{self, OfficialComparison};
    let (_directory, store, history) = demo();
    let proposed = scenario(&history);
    let mut expected = history[1].revision.clone();
    expected.source.confidence = Confidence::Official;
    let make = |expected: &Revision| OfficialComparison {
        schema_version: SCHEMA_VERSION,
        scenario: proposed.clone(),
        expected_versions: BTreeMap::from([(
            "DEMO_ACT".into(),
            store
                .save_revision(expected, instant("2026-02-01T00:00:00Z"))
                .unwrap(),
        )]),
    };
    let original = make(&expected);
    assert!(
        reconstruction::compare(&store, &original)
            .unwrap()
            .diagnostics
            .is_empty()
    );
    for variant in ["law", "confidence"] {
        let mut invalid = expected.clone();
        if variant == "law" {
            invalid.law_id = "OTHER_ACT".into();
        } else {
            invalid.source.confidence = Confidence::Synthetic;
        }
        assert!(reconstruction::compare(&store, &make(&invalid)).is_err());
    }
    for variant in ["repeal", "termination"] {
        let mut differing = expected.clone();
        if variant == "repeal" {
            differing.repeal = Some(day("1970-04-02"));
        } else {
            differing.termination = Some(Termination {
                status: TerminationStatus::Expired,
                date: LegalDate::Uncertain {
                    original: "Fixture expiry".into(),
                    reason: "Fixture uncertainty".into(),
                },
            });
        }
        let result = reconstruction::compare(&store, &make(&differing)).unwrap();
        assert_eq!(result.diagnostics[0].code, DiagnosticCode::OfficialMismatch);
        assert!(result.matched_versions.is_empty());
    }
    let reconstruction = reconstruction::Reconstruction {
        schema_version: SCHEMA_VERSION,
        scenario: proposed,
        revision_ids: BTreeMap::from([("DEMO_ACT".into(), "DERIVED_REVISION".into())]),
        evidence: history[0].revision.source.clone(),
    };
    let bytes = serde_json::to_vec(&reconstruction).unwrap();
    let versions =
        reconstruction::reconstruct(&store, &bytes, instant("2026-02-02T00:00:00Z")).unwrap();
    let derived = store.version(&versions[0]).unwrap();
    assert_eq!(
        derived.source.raw_sha256,
        law_history::store::sha256(&bytes)
    );
    assert_eq!(derived.source.locator, "/scenario/results/DEMO_ACT");
    assert_eq!(store.source(&derived.source.raw_sha256).unwrap(), bytes);
}

#[test]
fn machine_comparison_rows_check_original_elements_and_empty_rows() {
    let (_directory, _store, history) = demo();
    let path = "/Law/LawBody[index=1]/MainProvision[index=1]/Article[num=1]";
    let old = xml::index(&history[0].revision.tree)[path].clone();
    let replacement = xml::index(&history[1].revision.tree)[path].clone();
    let row = amendment::ComparisonRow {
        law_id: "DEMO_ACT".into(),
        path: path.into(),
        before: Some(old),
        after: Some(replacement.clone()),
        evidence: history[0].revision.source.clone(),
    };
    let operations = amendment::comparison_operations(std::slice::from_ref(&row)).unwrap();
    let result = amendment::apply_operation(&history[0].revision.tree, &operations[0]).unwrap();
    assert_eq!(xml::index(&result)[path], &replacement);
    assert_eq!(
        amendment::apply_operation(&history[1].revision.tree, &operations[0])
            .unwrap_err()
            .code,
        DiagnosticCode::OriginalMismatch
    );
    let empty = amendment::ComparisonRow {
        before: None,
        after: None,
        ..row
    };
    assert_eq!(
        amendment::comparison_operations(&[empty]).unwrap_err().code,
        DiagnosticCode::InvalidInput
    );
}

#[test]
fn blame_distinguishes_introduction_from_earliest_known_text() {
    let (_directory, store, history) = demo();
    let changed = "/Law/LawBody[index=1]/MainProvision[index=1]/Article[num=1]";
    let origin = service::blame(
        &store,
        "DEMO_ACT",
        changed,
        &Query::default(),
        day("2026-11-05"),
    )
    .unwrap();
    assert_eq!(origin.version, history[1].version);
    assert_eq!(origin.previous_version, Some(history[0].version.clone()));
    let mut incomplete = history[0].revision.clone();
    incomplete.law_id = "INCOMPLETE_ACT".into();
    incomplete.revision_id = "INCOMPLETE_ACT_EARLIEST".into();
    incomplete.original_enactment = false;
    store
        .save_revision(&incomplete, instant("2026-01-01T00:00:00Z"))
        .unwrap();
    let earliest = service::blame(
        &store,
        "INCOMPLETE_ACT",
        changed,
        &Query::default(),
        day("2026-11-05"),
    )
    .unwrap();
    assert_eq!(earliest.diagnostics[0].code, DiagnosticCode::CoverageGap);
}

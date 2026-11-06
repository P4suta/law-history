use base64::Engine;
use chrono::{DateTime, Utc};
use law_history::{Store, ingest, memory_store, model::*, service, store_types::Repository};
use law_history::{browser_ingest, ingest::LawInventory};
use serde_json::{Value, json};

fn populated() -> (tempfile::TempDir, Store, memory_store::Store) {
    let directory = tempfile::tempdir().unwrap();
    let native = Store::open(directory.path()).unwrap();
    let browser = memory_store::Store::default();
    let fixture = include_bytes!("../../../fixtures/demo.json");
    ingest::import_bundle(&native, fixture).unwrap();
    ingest::import_bundle(&browser, fixture).unwrap();
    (directory, native, browser)
}

fn projection(store: &impl Repository, cutoff: Option<DateTime<Utc>>) -> Value {
    let day = "1970-04-01".parse().unwrap();
    let query = Query {
        known_at: cutoff,
        ..Query::default()
    };
    json!({
        "ids":store.law_ids().unwrap(),
        "history":store.history("DEMO_ACT", cutoff).unwrap(),
        "search":service::search(store,"DEMO",&query,day).unwrap(),
        "keys":store.fact_keys("fact:").unwrap(),
        "facts":store.facts_with_prefix::<Value>("fact:",cutoff).unwrap(),
    })
}

#[test]
fn memory_store_matches_native_observations_facts_and_recovery() {
    let (_directory, native, browser) = populated();
    let history = native.history("DEMO_ACT", None).unwrap();
    let revisions: Vec<_> = history.iter().map(|row| row.revision.clone()).collect();
    let batch_at = "1970-01-01T00:00:00Z".parse().unwrap();
    let batch = browser.save_revisions(&revisions, batch_at).unwrap();
    assert_eq!(batch, native.save_revisions(&revisions, batch_at).unwrap());
    assert_eq!(
        batch,
        history
            .iter()
            .map(|row| row.version.clone())
            .collect::<Vec<_>>()
    );
    for time in [
        "1969-12-31T23:59:59.999999999Z",
        "1970-01-01T00:00:00Z",
        "2026-01-02T00:00:00Z",
    ] {
        let date: DateTime<Utc> = time.parse().unwrap();
        for value in [json!({"value":1}), json!({"value":2})] {
            native.fact("fact:same", &value, date).unwrap();
            browser.fact("fact:same", &value, date).unwrap();
        }
        native.fact("fact:unrelated", &json!(time), date).unwrap();
        browser.fact("fact:unrelated", &json!(time), date).unwrap();
        native.save_revision(&history[0].revision, date).unwrap();
        browser.save_revision(&history[0].revision, date).unwrap();
    }
    let recovered = memory_store::Store::default();
    recovered.restore(&browser.export().unwrap()).unwrap();
    for cutoff in [
        None,
        Some("1969-12-31T23:59:59.999999998Z".parse().unwrap()),
        Some("1969-12-31T23:59:59.999999999Z".parse().unwrap()),
        Some("1970-01-01T00:00:00Z".parse().unwrap()),
        Some("2026-01-01T00:00:00Z".parse().unwrap()),
        Some("2026-01-02T00:00:00Z".parse().unwrap()),
    ] {
        assert_eq!(projection(&native, cutoff), projection(&browser, cutoff));
        assert_eq!(projection(&browser, cutoff), projection(&recovered, cutoff));
        assert_eq!(
            native.indexed_history("DEMO_ACT", cutoff).unwrap(),
            browser.indexed_history("DEMO_ACT", cutoff).unwrap()
        );
    }
    assert_eq!(
        browser
            .source(&history[0].revision.source.raw_sha256)
            .unwrap(),
        native
            .source(&history[0].revision.source.raw_sha256)
            .unwrap()
    );
    assert_eq!(
        service::diff(&native, &history[0].version, &history[1].version)
            .unwrap()
            .changes
            .len(),
        service::diff(&browser, &history[0].version, &history[1].version)
            .unwrap()
            .changes
            .len()
    );
    assert_eq!(
        serde_json::to_value(
            service::refs(
                &native,
                "DEMO_ACT",
                &Query::default(),
                "1970-04-01".parse().unwrap()
            )
            .unwrap()
        )
        .unwrap(),
        serde_json::to_value(
            service::refs(
                &browser,
                "DEMO_ACT",
                &Query::default(),
                "1970-04-01".parse().unwrap()
            )
            .unwrap()
        )
        .unwrap()
    );
}

#[test]
fn snapshot_restore_is_atomic_for_corrupt_missing_and_mismatched_objects() {
    let (_directory, _native, browser) = populated();
    let original = browser.export().unwrap();
    let value: Value = serde_json::from_slice(&original).unwrap();
    for category in ["sources", "versions", "records"] {
        let mut corrupt = value.clone();
        let digest = corrupt[category]
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        corrupt[category][&digest] = json!([0]);
        assert!(
            browser
                .restore(&serde_json::to_vec(&corrupt).unwrap())
                .is_err()
        );
        assert_eq!(browser.export().unwrap(), original);
        if category != "records" {
            let mut missing = value.clone();
            missing[category] = json!({});
            assert!(
                browser
                    .restore(&serde_json::to_vec(&missing).unwrap())
                    .is_err()
            );
            assert_eq!(browser.export().unwrap(), original);
        }
    }
    for invalid in [b"null".as_slice(), b"{", b"{}", b"[]"] {
        assert!(browser.restore(invalid).is_err());
        assert_eq!(browser.export().unwrap(), original);
    }
    let mut unsupported = value;
    unsupported["schema_version"] = json!(2);
    assert!(
        browser
            .restore(&serde_json::to_vec(&unsupported).unwrap())
            .is_err()
    );
    assert_eq!(browser.export().unwrap(), original);
    assert!(browser.version(&"A".repeat(64)).is_err());
    assert!(browser.source("../sources").is_err());
}

#[test]
fn rejected_batches_and_missing_sources_never_publish_observations() {
    let (_directory, _native, browser) = populated();
    let mut revision = browser.history("DEMO_ACT", None).unwrap()[0]
        .revision
        .clone();
    let original = browser.export().unwrap();
    revision.source.raw_sha256 = "a".repeat(64);
    assert!(browser.save_revision(&revision, Utc::now()).is_err());
    assert!(browser.save_revisions(&[revision], Utc::now()).is_err());
    assert!(browser.save_revisions(&[], Utc::now()).is_err());
    assert_eq!(browser.export().unwrap(), original);
}

fn envelope() -> Value {
    json!({
        "law_info":{"law_id":"TEST_ACT","law_num":"Synthetic act","promulgation_date":"1969-01-01"},
        "revision_info":{"law_revision_id":"TEST_REVISION","law_title":"Synthetic title","updated":"2026-11-05T00:00:00Z","amendment_enforcement_date":"1970-04-01","amendment_enforcement_comment":"Confirmed in fixture","amendment_type":"1","mission":"New","repeal_status":"None","current_revision_status":"CurrentEnforced","amendment_law_id":"AMENDING_ACT"},
        "law_full_text":base64::engine::general_purpose::STANDARD.encode(b"<Law><LawBody><MainProvision><Article Num=\"1\"><Sentence>Fixture</Sentence></Article></MainProvision></LawBody></Law>")
    })
}

fn list(data: &Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"law_info":data["law_info"],"revisions":[data["revision_info"]]}))
        .unwrap()
}

#[test]
fn browser_collection_preserves_inventory_identity_dates_and_both_sources() {
    let store = memory_store::Store::default();
    let data = envelope();
    let bytes = list(&data);
    let started: DateTime<Utc> = "2026-11-06T00:00:00Z".parse().unwrap();
    let completed: DateTime<Utc> = "2026-11-06T00:00:01Z".parse().unwrap();
    let inventory = browser_ingest::history(&store, "TEST_ACT", &bytes, started).unwrap();
    assert_eq!(inventory.law_id, "TEST_ACT");
    assert_eq!(inventory.available_revisions, 1);
    assert_eq!(inventory.revision_ids, ["TEST_REVISION"]);
    assert_eq!(
        inventory.current_revision_id.as_deref(),
        Some("TEST_REVISION")
    );
    assert_eq!(inventory.current_snapshot_at, Some(started));
    assert_eq!(inventory.collected_at, started);
    assert_eq!(inventory.current_version, None);
    assert_eq!(inventory.source_digest, law_history::store::sha256(&bytes));
    let body = serde_json::to_vec(&data).unwrap();
    let version = browser_ingest::revision(
        &store,
        "TEST_ACT",
        "TEST_REVISION",
        &inventory.source_digest,
        &body,
        completed,
    )
    .unwrap();
    let saved = store.history("TEST_ACT", None).unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].version, version);
    assert_eq!(saved[0].observed_at, completed);
    assert_eq!(saved[0].revision.law_id, "TEST_ACT");
    assert_eq!(saved[0].revision.revision_id, "TEST_REVISION");
    assert_eq!(
        store.source(&saved[0].revision.source.raw_sha256).unwrap(),
        body
    );
    assert_eq!(
        saved[0].revision.source.url,
        "https://laws.e-gov.go.jp/api/2/law_data/TEST_REVISION?law_full_text_format=xml"
    );
    let metadata = saved[0].revision.metadata_source.as_ref().unwrap();
    assert_eq!(store.source(&metadata.raw_sha256).unwrap(), bytes);
    assert_eq!(metadata.locator, "/law_info, /revisions/0");
    let latest: LawInventory = store
        .latest_fact("inventory:TEST_ACT", None)
        .unwrap()
        .unwrap();
    assert_eq!(latest.current_version.as_deref(), Some(version.as_str()));
    assert_eq!(latest.current_snapshot_at, Some(started));
    assert_eq!(latest.collected_at, completed);
    assert!(
        store
            .latest_fact::<LawInventory>("inventory:TEST_ACT", Some(started))
            .unwrap()
            .unwrap()
            .current_version
            .is_none()
    );
}

#[test]
fn browser_current_markers_require_exactly_one_matching_revision() {
    for statuses in [
        vec!["PreviousEnforced"],
        vec!["CurrentEnforced"],
        vec!["CurrentEnforced", "PreviousEnforced"],
        vec!["PreviousEnforced", "CurrentEnforced"],
        vec!["CurrentEnforced", "CurrentEnforced"],
    ] {
        let store = memory_store::Store::default();
        let now: DateTime<Utc> = "2026-11-06T00:00:00Z".parse().unwrap();
        let data = envelope();
        let rows: Vec<_> = statuses
            .iter()
            .enumerate()
            .map(|(index, status)| {
                let mut row = data["revision_info"].clone();
                row["law_revision_id"] = json!(format!("REVISION_{index}"));
                row["current_revision_status"] = json!(status);
                row
            })
            .collect();
        let bytes =
            serde_json::to_vec(&json!({"law_info":data["law_info"],"revisions":rows})).unwrap();
        let inventory = browser_ingest::history(&store, "TEST_ACT", &bytes, now).unwrap();
        let expected = if statuses
            .iter()
            .filter(|status| **status == "CurrentEnforced")
            .count()
            == 1
        {
            Some(format!(
                "REVISION_{}",
                statuses
                    .iter()
                    .position(|status| *status == "CurrentEnforced")
                    .unwrap()
            ))
        } else {
            None
        };
        assert_eq!(inventory.current_revision_id, expected);
        for (index, row) in rows.iter().enumerate() {
            let mut body = data.clone();
            body["revision_info"] = row.clone();
            let revision = format!("REVISION_{index}");
            let version = browser_ingest::revision(
                &store,
                "TEST_ACT",
                &revision,
                &inventory.source_digest,
                &serde_json::to_vec(&body).unwrap(),
                now + chrono::Duration::seconds(index as i64 + 1),
            )
            .unwrap();
            let latest: LawInventory = store
                .latest_fact("inventory:TEST_ACT", None)
                .unwrap()
                .unwrap();
            if expected.as_deref() == Some(revision.as_str()) {
                assert_eq!(latest.current_version, Some(version));
            } else if expected.as_deref().is_none_or(|id| id > revision.as_str()) {
                assert_eq!(latest.current_version, None);
            }
        }
    }
}

#[test]
fn browser_collection_refuses_missing_changed_ambiguous_and_cross_law_sources() {
    let store = memory_store::Store::default();
    let now: DateTime<Utc> = "2026-11-06T00:00:00Z".parse().unwrap();
    let data = envelope();
    let body = serde_json::to_vec(&data).unwrap();
    assert!(
        browser_ingest::revision(&store, "TEST_ACT", "TEST_REVISION", "absent", &body, now)
            .is_err()
    );
    for invalid in [
        json!({"law_info":data["law_info"],"revisions":[]}),
        json!({"law_info":data["law_info"],"revisions":[data["revision_info"],data["revision_info"]]}),
        json!({"law_info":{"law_id":"OTHER_ACT","law_num":"Other","promulgation_date":null},"revisions":[data["revision_info"]]}),
    ] {
        assert!(
            browser_ingest::history(
                &store,
                "TEST_ACT",
                &serde_json::to_vec(&invalid).unwrap(),
                now
            )
            .is_err()
        );
        assert!(
            store
                .latest_fact::<LawInventory>("inventory:TEST_ACT", None)
                .unwrap()
                .is_none()
        );
    }
    assert!(browser_ingest::history(&store, "../law", &list(&data), now).is_err());
    let original = browser_ingest::history(&store, "TEST_ACT", &list(&data), now).unwrap();
    for (law, revision, digest) in [
        (
            "TEST_ACT",
            "OTHER_REVISION",
            original.source_digest.as_str(),
        ),
        ("TEST_ACT", "../revision", original.source_digest.as_str()),
        ("../law", "TEST_REVISION", original.source_digest.as_str()),
        ("TEST_ACT", "TEST_REVISION", "different"),
    ] {
        assert!(browser_ingest::revision(&store, law, revision, digest, &body, now).is_err());
        assert!(store.history("TEST_ACT", None).unwrap().is_empty());
    }
    let mut changed = data.clone();
    changed["revision_info"]["updated"] = json!("2026-11-06T00:00:00Z");
    let fresh = browser_ingest::history(
        &store,
        "TEST_ACT",
        &list(&changed),
        now + chrono::Duration::seconds(1),
    )
    .unwrap();
    assert_ne!(fresh.source_digest, original.source_digest);
    assert!(
        browser_ingest::revision(
            &store,
            "TEST_ACT",
            "TEST_REVISION",
            &original.source_digest,
            &body,
            now + chrono::Duration::seconds(2)
        )
        .is_err()
    );
    let mut wrong = changed;
    wrong["law_info"]["law_id"] = json!("OTHER_ACT");
    assert!(
        browser_ingest::revision(
            &store,
            "TEST_ACT",
            "TEST_REVISION",
            &fresh.source_digest,
            &serde_json::to_vec(&wrong).unwrap(),
            now + chrono::Duration::seconds(3)
        )
        .is_err()
    );
    assert!(store.history("TEST_ACT", None).unwrap().is_empty());
}

#[test]
fn browser_collection_rejects_cross_law_inventory_facts_before_saving_a_revision() {
    let store = memory_store::Store::default();
    let now = "2026-11-06T00:00:00Z".parse().unwrap();
    let mut data = envelope();
    data["law_info"]["law_id"] = json!("OTHER_ACT");
    let inventory = browser_ingest::history(&store, "OTHER_ACT", &list(&data), now).unwrap();
    for (index, law_id) in ["OTHER_ACT", "TEST_ACT"].into_iter().enumerate() {
        let mut fact = inventory.clone();
        fact.law_id = law_id.into();
        let observed_at = now + chrono::Duration::seconds(index as i64);
        store
            .fact("inventory:TEST_ACT", &fact, observed_at)
            .unwrap();
        assert!(
            browser_ingest::revision(
                &store,
                "TEST_ACT",
                "TEST_REVISION",
                &inventory.source_digest,
                &serde_json::to_vec(&data).unwrap(),
                observed_at
            )
            .is_err()
        );
    }
    assert!(store.law_ids().unwrap().is_empty());
}

#[test]
fn snapshot_records_validate_each_schema_reference_and_batch_condition() {
    let (_directory, _native, store) = populated();
    let before = store.export().unwrap();
    let snapshot: Value = serde_json::from_slice(&before).unwrap();
    let version = store.history("DEMO_ACT", None).unwrap()[0].version.clone();
    let valid = json!({"schema_version":1,"version":version,"observed_at":"1969-12-31T23:59:59Z"});
    let mut bad_schema = valid.clone();
    bad_schema["schema_version"] = json!(0);
    let mut missing = valid.clone();
    missing["version"] = json!("a".repeat(64));
    for record in [
        json!({"kind":"observation","schema_version":0,"version":version,"observed_at":"1969-12-31T23:59:59Z"}),
        json!({"kind":"observation","schema_version":1,"version":"a".repeat(64),"observed_at":"1969-12-31T23:59:59Z"}),
        json!({"kind":"batch","schema_version":0,"observations":[valid]}),
        json!({"kind":"batch","schema_version":1,"observations":[]}),
        json!({"kind":"batch","schema_version":1,"observations":[bad_schema]}),
        json!({"kind":"batch","schema_version":1,"observations":[missing]}),
        json!({"kind":"fact","schema_version":0,"key":"fact:broken","value":null,"observed_at":"1969-12-31T23:59:59Z"}),
        json!({"kind":"unknown"}),
    ] {
        let mut candidate = snapshot.clone();
        let bytes = serde_json::to_vec(&record).unwrap();
        candidate["records"]
            .as_object_mut()
            .unwrap()
            .insert(law_history::store::sha256(&bytes), json!(bytes));
        assert!(
            store
                .restore(&serde_json::to_vec(&candidate).unwrap())
                .is_err(),
            "{record}"
        );
        assert_eq!(store.export().unwrap(), before);
    }
    let mut invalid_version = snapshot.clone();
    let versions = invalid_version["versions"].as_object_mut().unwrap();
    let bytes: Vec<u8> = serde_json::from_value(versions.values().next().unwrap().clone()).unwrap();
    let mut revision: Value = serde_json::from_slice(&bytes).unwrap();
    revision["schema_version"] = json!(0);
    let bytes = serde_json::to_vec(&revision).unwrap();
    versions.insert(law_history::store::sha256(&bytes), json!(bytes));
    assert!(
        store
            .restore(&serde_json::to_vec(&invalid_version).unwrap())
            .is_err()
    );
    assert_eq!(store.export().unwrap(), before);
}

#[test]
fn content_objects_and_snapshot_inputs_include_their_exact_size_boundaries() {
    let store = memory_store::Store::default();
    let mut source = vec![0; 64 * 1024 * 1024];
    let digest = store.preserve(&source).unwrap();
    assert_eq!(digest, law_history::store::sha256(&source));
    source.push(0);
    assert!(
        store
            .preserve(&source)
            .unwrap_err()
            .to_string()
            .contains("64 MiB")
    );
    drop(source);
    drop(store);
    let store = memory_store::Store::default();
    let empty = store.export().unwrap();
    let mut bytes = vec![b' '; 256 * 1024 * 1024];
    bytes[..empty.len()].copy_from_slice(&empty);
    store.restore(&bytes).unwrap();
    bytes.push(b' ');
    assert!(
        store
            .restore(&bytes)
            .unwrap_err()
            .to_string()
            .contains("256 MiB")
    );
    assert_eq!(store.export().unwrap(), empty);
}

#[test]
fn law_identity_inventory_does_not_apply_a_history_knowledge_cutoff() {
    let (_directory, native, browser) = populated();
    let mut revision = native.history("DEMO_ACT", None).unwrap()[0]
        .revision
        .clone();
    revision.law_id = "LEAP_ACT".into();
    revision.revision_id = "LEAP_REVISION".into();
    let leap = "9999-12-31T23:59:60Z".parse().unwrap();
    native.save_revision(&revision, leap).unwrap();
    browser.save_revision(&revision, leap).unwrap();
    assert_eq!(browser.law_ids().unwrap(), native.law_ids().unwrap());
    assert_eq!(
        browser.history("LEAP_ACT", None).unwrap().len(),
        native.history("LEAP_ACT", None).unwrap().len()
    );
}

#[test]
fn fact_prefixes_match_sqlite_at_unicode_delimiters_and_embedded_nulls() {
    let directory = tempfile::tempdir().unwrap();
    let native = Store::open(directory.path()).unwrap();
    let browser = memory_store::Store::default();
    let keys = [
        "",
        "fact",
        "fact:",
        "fact:one",
        "fact:\0",
        "fact:\0hidden",
        "日",
        "日本",
        "日本:1",
        "\0",
        "\0suffix",
        "fact:💡",
        "fact:%",
        "fact:_",
    ];
    let mut prefixes = std::collections::BTreeSet::new();
    for key in keys {
        for (index, time) in ["1969-12-31T23:59:59Z", "1970-01-01T00:00:00Z"]
            .into_iter()
            .enumerate()
        {
            let value = json!({"key":key,"value":index});
            native.fact(key, &value, time.parse().unwrap()).unwrap();
            browser.fact(key, &value, time.parse().unwrap()).unwrap();
        }
        prefixes.insert(key);
        for (offset, _) in key.char_indices() {
            prefixes.insert(&key[..offset]);
        }
    }
    for prefix in prefixes {
        assert_eq!(
            browser.fact_keys(prefix).unwrap(),
            native.fact_keys(prefix).unwrap(),
            "prefix {prefix:?}"
        );
        for cutoff in [
            None,
            Some("1969-12-31T23:59:59Z".parse().unwrap()),
            Some("1969-12-31T23:59:58Z".parse().unwrap()),
        ] {
            assert_eq!(
                browser.facts_with_prefix::<Value>(prefix, cutoff).unwrap(),
                native.facts_with_prefix::<Value>(prefix, cutoff).unwrap(),
                "prefix {prefix:?}"
            );
        }
    }
}

#[test]
fn catalog_titles_share_the_revision_projection_and_validate_counts_and_identities() {
    let row = json!({"law_info":{"law_id":"TITLE_ACT","law_num":"Synthetic act"},"revision_info":{"law_title":"<Ruby>法<Rt>ほう</Rt></Ruby>律"}});
    let page = |total: usize, laws: Vec<Value>| {
        serde_json::to_vec(&json!({"total_count":total,"laws":laws})).unwrap()
    };
    let result = browser_ingest::catalog(&page(1, vec![row.clone()])).unwrap();
    assert_eq!(result.total_count, 1);
    assert_eq!(result.laws[0].law_info.law_id, "TITLE_ACT");
    assert_eq!(result.laws[0].revision_info.law_title, "法律");
    assert!(
        browser_ingest::catalog(&page(0, vec![]))
            .unwrap()
            .laws
            .is_empty()
    );
    assert!(browser_ingest::catalog(&page(0, vec![row.clone()])).is_err());
    assert!(browser_ingest::catalog(&page(2, vec![row.clone(), row.clone()])).is_err());
    let rows: Vec<_> = (0..21)
        .map(|index| {
            let mut value = row.clone();
            value["law_info"]["law_id"] = json!(format!("TITLE_{index}"));
            value
        })
        .collect();
    assert_eq!(
        browser_ingest::catalog(&page(20, rows[..20].to_vec()))
            .unwrap()
            .laws
            .len(),
        20
    );
    assert!(browser_ingest::catalog(&page(21, rows)).is_err());
    let mut invalid = row.clone();
    invalid["law_info"]["law_id"] = json!("../law");
    assert!(browser_ingest::catalog(&page(1, vec![invalid])).is_err());
    for title in ["", "  ", "<script>invalid</script>"] {
        let mut invalid = row.clone();
        invalid["revision_info"]["law_title"] = json!(title);
        assert!(browser_ingest::catalog(&page(1, vec![invalid])).is_err());
    }
    assert!(browser_ingest::catalog(b"{}").is_err());
}

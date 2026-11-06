use base64::Engine;
use chrono::{DateTime, Utc};
use law_history::{
    Store,
    ingest::{self, EGOV_API, Fetcher},
    model::*,
};
use serde_json::{Value, json};
use std::time::Duration;

fn now() -> DateTime<Utc> {
    "2026-11-05T00:00:00Z".parse().unwrap()
}

fn cached_fetcher() -> Fetcher {
    Fetcher::with_clock(Duration::ZERO, Some(0), now).unwrap()
}

#[test]
fn current_snapshot_binding_survives_receipt_reuse_for_each_marker_position_and_absence() {
    for statuses in [
        ["CurrentEnforced", "PreviousEnforced"],
        ["PreviousEnforced", "CurrentEnforced"],
        ["PreviousEnforced", "PreviousEnforced"],
        ["CurrentEnforced", "CurrentEnforced"],
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let mut entries = Vec::new();
        let mut law = Value::Null;
        for (index, status) in statuses.iter().enumerate() {
            let mut data = envelope();
            data["revision_info"]["law_revision_id"] = json!(format!("TEST_REVISION_{index}"));
            data["revision_info"]["current_revision_status"] = json!(status);
            data["revision_info"]["amendment_enforcement_date"] = json!("1970-04-01");
            let id = data["revision_info"]["law_revision_id"].as_str().unwrap();
            cache(
                &store,
                &format!("{EGOV_API}/law_data/{id}?law_full_text_format=xml"),
                &data,
            );
            entries.push(data["revision_info"].clone());
            law = data["law_info"].clone();
        }
        cache(
            &store,
            &format!("{EGOV_API}/law_revisions/TEST_ACT"),
            &json!({"law_info":law,"revisions":entries}),
        );
        let current: Vec<_> = statuses
            .iter()
            .enumerate()
            .filter(|(_, status)| **status == "CurrentEnforced")
            .map(|(index, _)| format!("TEST_REVISION_{index}"))
            .collect();
        for pass in 0..2 {
            let observed = now() + chrono::Duration::seconds(pass);
            let mut fetcher =
                Fetcher::with_clock(Duration::ZERO, Some(0), move || observed).unwrap();
            let report = ingest::sync_law(&store, &mut fetcher, "TEST_ACT", false).unwrap();
            assert!(
                report.diagnostics.is_empty(),
                "{statuses:?}: {:?}",
                report.diagnostics
            );
            assert_eq!(
                (report.saved, report.reused),
                if pass == 0 { (2, 0) } else { (0, 2) }
            );
            let inventory: ingest::LawInventory = store
                .latest_fact("inventory:TEST_ACT", None)
                .unwrap()
                .unwrap();
            if current.len() == 1 {
                let expected = store
                    .history("TEST_ACT", None)
                    .unwrap()
                    .into_iter()
                    .find(|item| item.revision.revision_id == current[0])
                    .unwrap()
                    .version;
                assert_eq!(
                    inventory.current_version.as_deref(),
                    Some(expected.as_str()),
                    "{statuses:?}, pass {pass}"
                );
                assert_eq!(
                    law_history::service::select(
                        &store,
                        "TEST_ACT",
                        &Query::default(),
                        "2026-11-05".parse().unwrap()
                    )
                    .unwrap()
                    .selected
                    .version,
                    expected
                );
            } else {
                assert!(inventory.current_revision_id.is_none());
                assert!(
                    inventory.current_version.is_none(),
                    "{statuses:?}, pass {pass}"
                );
                assert!(matches!(
                    law_history::service::select(
                        &store,
                        "TEST_ACT",
                        &Query::default(),
                        "2026-11-05".parse().unwrap()
                    ),
                    Err(HistoryError::Ambiguous(_))
                ));
            }
            assert!(matches!(
                law_history::service::select(
                    &store,
                    "TEST_ACT",
                    &Query {
                        at: Some("1970-04-01".parse().unwrap()),
                        ..Default::default()
                    },
                    "2026-11-05".parse().unwrap()
                ),
                Err(HistoryError::Ambiguous(_))
            ));
        }
    }
}

fn cache(store: &Store, url: &str, value: &Value) {
    cache_at(store, url, value, now());
}

fn cache_at(store: &Store, url: &str, value: &Value, collected: DateTime<Utc>) {
    cache_bytes_at(store, url, &serde_json::to_vec(value).unwrap(), collected);
}

fn cache_bytes_at(store: &Store, url: &str, bytes: &[u8], collected: DateTime<Utc>) {
    let digest = store.preserve(bytes).unwrap();
    store
        .fact(
            &format!("cache:{}", law_history::store::sha256(url.as_bytes())),
            &json!({"digest":digest,"collected_at":collected,"acquired_at":collected}),
            collected,
        )
        .unwrap();
}

#[test]
fn official_ruby_titles_agree_without_losing_either_source_or_tree_annotations() {
    let history_bytes = include_bytes!("../../../fixtures/egov-ruby-history.json");
    let body_bytes = include_bytes!("../../../fixtures/egov-ruby-body.json");
    let fixture: Value =
        serde_json::from_slice(include_bytes!("../../../fixtures/egov-ruby-title.json")).unwrap();
    assert_eq!(
        law_history::store::sha256(history_bytes),
        fixture["history_sha256"]
    );
    assert_eq!(
        law_history::store::sha256(body_bytes),
        fixture["body_sha256"]
    );
    let history: Value = serde_json::from_slice(history_bytes).unwrap();
    let body: Value = serde_json::from_slice(body_bytes).unwrap();
    assert_eq!(history["revisions"][0]["law_title"], fixture["plain_title"]);
    assert_eq!(body["revision_info"]["law_title"], fixture["inline_title"]);
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let snapshot = ingest::HistorySnapshot::from_response(
        &store,
        history_bytes,
        &format!("{EGOV_API}/law_revisions/337AC0000000150"),
    )
    .unwrap();
    let url = format!(
        "{EGOV_API}/law_data/337AC0000000150_20251001_504AC0000000104?law_full_text_format=xml"
    );
    cache_bytes_at(&store, &url, body_bytes, now());
    let revision = cached_fetcher()
        .fetch_revision(&store, &url, false, snapshot.revision(0).unwrap())
        .unwrap();
    assert_eq!(
        revision.title,
        "激甚災害に対処するための特別の財政援助等に関する法律"
    );
    assert_eq!(
        store.source(&revision.source.raw_sha256).unwrap(),
        body_bytes
    );
    assert_eq!(
        store
            .source(&revision.metadata_source.as_ref().unwrap().raw_sha256)
            .unwrap(),
        history_bytes
    );
    assert!(
        law_history::xml::index(&revision.tree)
            .values()
            .any(|node| {
                node.tag == "Ruby"
                    && node
                        .children()
                        .any(|child| child.tag == "Rt" && child.text() == "じん")
            })
    );
    let standalone = ingest::revision_from_response(&store, body_bytes, &url).unwrap();
    assert_eq!(standalone.title, revision.title);
    let version = store.save_revision(&revision, now()).unwrap();
    store.rebuild().unwrap();
    assert!(store.version(&version).unwrap() == revision);
}

#[test]
fn title_agreement_compares_base_characters_and_keeps_each_raw_representation() {
    let base: ingest::RevisionInfo =
        serde_json::from_value(envelope()["revision_info"].clone()).unwrap();
    let variants = [
        ("甲乙法", "甲<Ruby>乙<Rt>第二条</Rt></Ruby>法"),
        (
            "甲乙法",
            "<Line Style=\"double\">甲<Ruby>乙<Rt>おつ</Rt></Ruby>法</Line>",
        ),
        ("甲 乙23法", "甲<Line> </Line>乙<Sup>2</Sup><Sub>3</Sub>法"),
        ("甲&乙法", "甲&amp;<Ruby>乙<Rt>おつ</Rt></Ruby>法"),
        ("甲乙法", "<Ruby>&#x7532;<Rt>こう</Rt></Ruby>乙法"),
    ];
    for (plain, marked) in variants {
        for (first, second) in [(plain, marked), (marked, plain)] {
            let mut a = base.metadata.clone();
            let mut b = base.metadata.clone();
            a.law_title = first.into();
            b.law_title = second.into();
            let merged = a.merged(&b).unwrap();
            assert_eq!(merged.law_title, first);
            assert_eq!(
                law_history::xml::title_text(&merged.law_title).unwrap(),
                plain
            );
            b.law_title = format!("{second}別");
            assert!(!a.agrees_with(&b), "{first:?} {second:?}");
        }
    }
    for absent in ["", " ", "<Ruby><Rt>第二条</Rt></Ruby>"] {
        let mut a = base.metadata.clone();
        let mut b = base.metadata.clone();
        a.law_title = absent.into();
        b.law_title = "甲<Ruby>乙<Rt>おつ</Rt></Ruby>法".into();
        assert_eq!(a.merged(&b).unwrap().law_title, b.law_title);
    }
    let template = base.metadata;
    for code in 0..27 {
        let characters = ['甲', '乙', '丙'];
        let base: String = (0..3)
            .map(|index| characters[(code / 3usize.pow(index)) % 3])
            .collect();
        for mask in 0..8 {
            for reading in ["よみ", "第二条", "甲法第三条"] {
                let decorated: String = base
                    .chars()
                    .enumerate()
                    .map(|(index, ch)| {
                        if mask & (1 << index) == 0 {
                            ch.to_string()
                        } else {
                            format!("<Ruby>{ch}<Rt>{reading}</Rt></Ruby>")
                        }
                    })
                    .collect();
                let mut a = template.clone();
                let mut b = template.clone();
                a.law_title = base.clone();
                b.law_title = decorated.clone();
                assert!(a.agrees_with(&b), "{base:?} {decorated:?}");
                assert!(b.agrees_with(&a));
                a.law_title.push('別');
                assert!(!a.agrees_with(&b));
            }
        }
    }
}

#[test]
fn cache_import_reobserves_verified_sources_without_copying_history_or_replacing_newer_cache() {
    let directory = tempfile::tempdir().unwrap();
    let archive = Store::open(directory.path().join("archive")).unwrap();
    let store = Store::open(directory.path().join("destination")).unwrap();
    let current = now() + chrono::Duration::minutes(1);
    cache_at(&archive, "at-boundary", &json!({"equal":true}), current);
    for (url, time) in [
        ("good", now()),
        ("old", now() - chrono::Duration::days(1)),
        ("future", now() + chrono::Duration::hours(1)),
        ("existing", now()),
    ] {
        cache_at(&archive, url, &json!({"url":url}), time);
    }
    cache_at(&store, "existing", &json!({"newer":true}), current);
    archive.fact("議案:one", &json!(1), now()).unwrap();
    archive.fact("議案extra", &json!(2), now()).unwrap();
    assert_eq!(archive.fact_keys("議案:").unwrap(), vec!["議案:one"]);
    assert_eq!(
        archive.facts_with_prefix::<Value>("議案:", None).unwrap(),
        vec![json!(1)]
    );
    let report = ingest::import_cache(&store, &archive, || current).unwrap();
    assert_eq!((report.imported, report.skipped), (2, 3));
    let mut expected: Vec<_> = ["at-boundary", "good", "existing"]
        .into_iter()
        .map(|url| format!("cache:{}", law_history::store::sha256(url.as_bytes())))
        .collect();
    expected.sort();
    assert_eq!(store.fact_keys("cache:").unwrap(), expected);
    assert!(store.law_ids().unwrap().is_empty());
    let key = format!("cache:{}", law_history::store::sha256(b"good"));
    assert!(
        store
            .latest_fact::<Value>(&key, Some(now()))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.latest_fact::<Value>(&key, None).unwrap().unwrap()["collected_at"],
        json!(current)
    );
    assert_eq!(
        store.latest_fact::<Value>(&key, None).unwrap().unwrap()["acquired_at"],
        json!(now())
    );
    assert_eq!(
        archive.latest_fact::<Value>(&key, None).unwrap().unwrap()["collected_at"],
        json!(now())
    );
    let origin = store
        .latest_fact::<Value>(&format!("archive-{key}"), None)
        .unwrap()
        .unwrap();
    assert_eq!(origin["previously_recorded_at"], json!(now()));
    assert_eq!(origin["reobserved_at"], json!(current));
    store.rebuild().unwrap();
    let mut fetcher = Fetcher::with_clock(Duration::ZERO, Some(0), move || current).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&fetcher.fetch(&store, "good", false).unwrap()).unwrap(),
        json!({"url":"good"})
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fetcher.fetch(&store, "existing", false).unwrap())
            .unwrap(),
        json!({"newer":true})
    );
    let repeated = ingest::import_cache(&store, &archive, || current).unwrap();
    assert_eq!((repeated.imported, repeated.skipped), (0, 5));
    assert!(
        ingest::import_cache(
            &store,
            &Store::open(store.root().join(".")).unwrap(),
            || current
        )
        .is_err()
    );
    cache(
        &archive,
        "broken",
        &json!({"body":"corrupt archive control"}),
    );
    let broken_key = format!("cache:{}", law_history::store::sha256(b"broken"));
    let cached = archive
        .latest_fact::<Value>(&broken_key, None)
        .unwrap()
        .unwrap();
    std::fs::write(
        archive
            .root()
            .join("sources")
            .join(cached["digest"].as_str().unwrap()),
        b"corrupt",
    )
    .unwrap();
    assert!(ingest::import_cache(&store, &archive, || current).is_err());
    assert!(
        store
            .latest_fact::<Value>(&broken_key, None)
            .unwrap()
            .is_none()
    );
}

#[test]
fn cache_import_checks_each_observation_time_without_balancing_wrong_members() {
    let current = now() + chrono::Duration::hours(12);
    for (collected_at, accepted) in [
        (now() - chrono::Duration::nanoseconds(1), false),
        (now(), true),
        (current - chrono::Duration::nanoseconds(1), true),
        (current, true),
        (current + chrono::Duration::nanoseconds(1), false),
        (now() + chrono::Duration::days(1), false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let archive = Store::open(directory.path().join("archive")).unwrap();
        let store = Store::open(directory.path().join("destination")).unwrap();
        let bytes = b"A preserved source for one observation boundary";
        cache_bytes_at(&archive, "boundary", bytes, collected_at);
        let report = ingest::import_cache(&store, &archive, || current).unwrap();
        let key = format!("cache:{}", law_history::store::sha256(b"boundary"));
        let imported = store.latest_fact::<Value>(&key, None).unwrap();
        assert_eq!(imported.is_some(), accepted, "{collected_at}");
        assert_eq!(report.imported, usize::from(accepted));
        assert_eq!(report.skipped, usize::from(!accepted));
        assert_eq!(
            store.fact_keys("cache:").unwrap().len(),
            usize::from(accepted)
        );
        if let Some(imported) = imported {
            assert_eq!(imported["collected_at"], json!(current));
            assert_eq!(imported["acquired_at"], json!(collected_at));
            assert_eq!(
                store.source(imported["digest"].as_str().unwrap()).unwrap(),
                bytes
            );
        }
    }
}

#[test]
fn legacy_cache_reobservation_never_acquires_a_current_snapshot_timestamp() {
    let directory = tempfile::tempdir().unwrap();
    let archive = Store::open(directory.path().join("archive")).unwrap();
    let store = Store::open(directory.path().join("destination")).unwrap();
    let data = envelope();
    let url = format!("{EGOV_API}/law_revisions/TEST_ACT");
    let key = format!("cache:{}", law_history::store::sha256(url.as_bytes()));
    let digest = archive
        .preserve(
            &serde_json::to_vec(
                &json!({"law_info":data["law_info"],"revisions":[data["revision_info"].clone()]}),
            )
            .unwrap(),
        )
        .unwrap();
    archive
        .fact(&key, &json!({"digest":digest,"collected_at":now()}), now())
        .unwrap();
    let later = now() + chrono::Duration::hours(1);
    assert_eq!(
        ingest::import_cache(&store, &archive, || later)
            .unwrap()
            .imported,
        1
    );
    let mut fetcher = Fetcher::with_clock(Duration::ZERO, Some(0), move || later).unwrap();
    let snapshot = fetcher.fetch_snapshot(&store, &url, false).unwrap();
    assert!(snapshot.acquired_at.is_none());
    cache_at(
        &store,
        &format!("{EGOV_API}/law_data/TEST_ACT_REVISION?law_full_text_format=xml"),
        &data,
        later,
    );
    assert_eq!(
        ingest::sync_law(&store, &mut fetcher, "TEST_ACT", false)
            .unwrap()
            .saved,
        1
    );
    let inventory: ingest::LawInventory = store
        .latest_fact("inventory:TEST_ACT", None)
        .unwrap()
        .unwrap();
    assert_eq!(
        inventory.current_revision_id.as_deref(),
        Some("TEST_ACT_REVISION")
    );
    assert!(inventory.current_snapshot_at.is_none());
    assert_eq!(inventory.collected_at, later);
}

#[test]
fn catalog_rejects_a_nonadvancing_cursor_before_revisiting_its_page() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    cache(
        &store,
        &format!("{EGOV_API}/laws?limit=1000&offset=0&order=%2Blaw_info.law_id"),
        &catalog_page(2, Some(0), &["ACT_A"]),
    );
    let error = ingest::catalog(&store, &mut cached_fetcher(), false).unwrap_err();
    assert!(error.to_string().contains("did not advance"));
}

#[test]
fn sync_receipts_resume_rebuild_refresh_metadata_and_repair_missing_versions() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let mut data = envelope();
    let list_url = format!("{EGOV_API}/law_revisions/TEST_ACT");
    let data_url = format!("{EGOV_API}/law_data/TEST_ACT_REVISION?law_full_text_format=xml");
    let list = |data: &Value| json!({"law_info":data["law_info"],"revisions":[data["revision_info"].clone()]});
    cache(&store, &list_url, &list(&data));
    cache(&store, &data_url, &data);
    let clock = std::sync::Arc::new(std::sync::Mutex::new(now()));
    let current = clock.clone();
    let mut fetcher =
        Fetcher::with_clock(Duration::ZERO, Some(0), move || *current.lock().unwrap()).unwrap();
    let first = ingest::sync_law(&store, &mut fetcher, "TEST_ACT", false).unwrap();
    assert_eq!((first.laws, first.saved, first.reused), (1, 1, 0));
    assert!(first.diagnostics.is_empty());
    let first_version = store.history("TEST_ACT", None).unwrap()[0].version.clone();
    store.rebuild().unwrap();
    *clock.lock().unwrap() = now() + chrono::Duration::seconds(1);
    let reused = ingest::sync_law(&store, &mut fetcher, "TEST_ACT", false).unwrap();
    assert_eq!((reused.laws, reused.saved, reused.reused), (1, 0, 1));
    let receipt_key = "receipt:TEST_ACT_REVISION";
    let receipt = store
        .latest_fact::<Value>(receipt_key, None)
        .unwrap()
        .unwrap();
    for (index, (field, value)) in [
        ("extractor_version", json!(0)),
        ("metadata_sha256", json!("stale")),
        ("version", json!("a".repeat(64))),
    ]
    .into_iter()
    .enumerate()
    {
        let time = now() + chrono::Duration::seconds(10 + index as i64 * 2);
        let mut invalid = receipt.clone();
        invalid[field] = value;
        store.fact(receipt_key, &invalid, time).unwrap();
        *clock.lock().unwrap() = time + chrono::Duration::seconds(1);
        let repaired = ingest::sync_law(&store, &mut fetcher, "TEST_ACT", false).unwrap();
        assert_eq!((repaired.saved, repaired.reused), (1, 0));
        assert!(repaired.diagnostics.is_empty());
    }
    let time = now() + chrono::Duration::hours(1);
    data["revision_info"]["law_title"] = json!("Corrected source title");
    data["revision_info"]["updated"] = json!("2026-11-05T01:00:00Z");
    cache_at(&store, &list_url, &list(&data), time);
    cache_at(&store, &data_url, &data, time);
    *clock.lock().unwrap() = time;
    let changed = ingest::sync_law(&store, &mut fetcher, "TEST_ACT", false).unwrap();
    assert_eq!((changed.saved, changed.reused), (1, 0));
    let latest = store
        .history("TEST_ACT", None)
        .unwrap()
        .into_iter()
        .max_by_key(|entry| entry.observed_at)
        .unwrap();
    assert_eq!(latest.revision.title, "Corrected source title");
    assert_ne!(latest.version, first_version);
    assert_eq!(
        store.version(&first_version).unwrap().title,
        "Synthetic title"
    );
}

#[test]
fn receipt_reuse_checks_the_preserved_law_and_revision_subjects() {
    for field in ["law", "revision"] {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let data = envelope();
        cache(
            &store,
            &format!("{EGOV_API}/law_revisions/TEST_ACT"),
            &json!({"law_info":data["law_info"],"revisions":[data["revision_info"].clone()]}),
        );
        cache(
            &store,
            &format!("{EGOV_API}/law_data/TEST_ACT_REVISION?law_full_text_format=xml"),
            &data,
        );
        ingest::sync_law(&store, &mut cached_fetcher(), "TEST_ACT", false).unwrap();
        let mut receipt = store
            .latest_fact::<Value>("receipt:TEST_ACT_REVISION", None)
            .unwrap()
            .unwrap();
        let original_version = receipt["version"].as_str().unwrap().to_owned();
        let mut foreign = store.version(&original_version).unwrap();
        if field == "law" {
            foreign.law_id = "OTHER_ACT".into();
        } else {
            foreign.revision_id = "OTHER_REVISION".into();
        }
        receipt["version"] = json!(store.save_revision(&foreign, now()).unwrap());
        let later = now() + chrono::Duration::seconds(1);
        store
            .fact("receipt:TEST_ACT_REVISION", &receipt, later)
            .unwrap();
        let mut fetcher = Fetcher::with_clock(Duration::ZERO, Some(0), move || later).unwrap();
        let result = ingest::sync_law(&store, &mut fetcher, "TEST_ACT", false).unwrap();
        assert_eq!((result.saved, result.reused), (1, 0), "{field}");
        assert!(result.diagnostics.is_empty());
        let inventory: ingest::LawInventory = store
            .latest_fact("inventory:TEST_ACT", None)
            .unwrap()
            .unwrap();
        assert_eq!(inventory.current_version, Some(original_version));
    }
}

#[test]
fn collection_observations_use_each_completed_stage_instead_of_the_batch_start() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let data = envelope();
    cache(
        &store,
        &format!("{EGOV_API}/law_revisions/TEST_ACT"),
        &json!({"law_info":data["law_info"],"revisions":[data["revision_info"].clone()]}),
    );
    cache(
        &store,
        &format!("{EGOV_API}/law_data/TEST_ACT_REVISION?law_full_text_format=xml"),
        &data,
    );
    let ticks = Arc::new(AtomicUsize::new(0));
    let current = ticks.clone();
    let mut fetcher = Fetcher::with_clock(Duration::ZERO, Some(0), move || {
        now() + chrono::Duration::seconds(current.fetch_add(1, Ordering::SeqCst) as i64)
    })
    .unwrap();
    let report = ingest::sync_law(&store, &mut fetcher, "TEST_ACT", false).unwrap();
    assert_eq!(report.saved, 1);
    assert!(store.history("TEST_ACT", Some(now())).unwrap().is_empty());
    let revision = store.history("TEST_ACT", None).unwrap().remove(0);
    let inventory: ingest::LawInventory = store
        .latest_fact("inventory:TEST_ACT", None)
        .unwrap()
        .unwrap();
    assert!(inventory.collected_at > now());
    assert_eq!(inventory.current_snapshot_at, Some(now()));
    assert!(revision.observed_at > inventory.collected_at);
    assert!(
        store
            .latest_fact::<Value>("receipt:TEST_ACT_REVISION", Some(inventory.collected_at))
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .latest_fact::<Value>("sync:TEST_ACT", Some(revision.observed_at))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .history("TEST_ACT", Some(revision.observed_at))
            .unwrap()
            .len(),
        1
    );
    store.rebuild().unwrap();
    assert!(store.history("TEST_ACT", Some(now())).unwrap().is_empty());
    assert_eq!(
        store
            .history("TEST_ACT", Some(revision.observed_at))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn sync_refuses_a_different_history_law_or_a_different_data_identity() {
    for field in ["history_law", "data_law", "data_revision"] {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let mut data = envelope();
        let mut list =
            json!({"law_info":data["law_info"],"revisions":[data["revision_info"].clone()]});
        match field {
            "history_law" => list["law_info"]["law_id"] = json!("OTHER_ACT"),
            "data_law" => data["law_info"]["law_id"] = json!("OTHER_ACT"),
            _ => data["revision_info"]["law_revision_id"] = json!("OTHER_REVISION"),
        }
        cache(&store, &format!("{EGOV_API}/law_revisions/TEST_ACT"), &list);
        cache(
            &store,
            &format!("{EGOV_API}/law_data/TEST_ACT_REVISION?law_full_text_format=xml"),
            &data,
        );
        let result = ingest::sync_law(&store, &mut cached_fetcher(), "TEST_ACT", false);
        if field == "history_law" {
            assert!(result.is_err());
        } else {
            let result = result.unwrap();
            assert_eq!(result.saved, 0);
            assert_eq!(result.diagnostics.len(), 1);
            assert!(result.diagnostics[0].message.contains("identity differs"));
        }
        assert!(store.history("TEST_ACT", None).unwrap().is_empty());
    }
}

fn law(id: &str) -> Value {
    json!({"law_info":{"law_id":id,"law_num":"Synthetic act","promulgation_date":"1969-01-01"}})
}

fn catalog_page(total: usize, next: Option<usize>, ids: &[&str]) -> Value {
    json!({"total_count":total,"next_offset":next,"laws":ids.iter().map(|id|law(id)).collect::<Vec<_>>()})
}

#[test]
fn catalog_pagination_checks_counts_uniqueness_progress_and_cached_sources() {
    for (pages, succeeds) in [
        (
            vec![
                catalog_page(2, Some(1), &["ACT_A"]),
                catalog_page(2, None, &["ACT_B"]),
            ],
            true,
        ),
        (vec![catalog_page(1, Some(1), &["ACT_A"])], true),
        (vec![catalog_page(1, Some(0), &["ACT_A"])], false),
        (
            vec![
                catalog_page(2, Some(1), &["ACT_A"]),
                catalog_page(3, None, &["ACT_B"]),
            ],
            false,
        ),
        (
            vec![
                catalog_page(2, Some(1), &["ACT_A"]),
                catalog_page(2, Some(1), &["ACT_B"]),
            ],
            false,
        ),
        (vec![catalog_page(2, Some(0), &["ACT_A"])], false),
        (vec![catalog_page(2, None, &["ACT_A", "ACT_A"])], false),
        (vec![catalog_page(1, None, &["invalid/path"])], false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        for (offset, page) in pages.iter().enumerate() {
            cache(
                &store,
                &format!("{EGOV_API}/laws?limit=1000&offset={offset}&order=%2Blaw_info.law_id"),
                page,
            );
        }
        let mut fetcher = cached_fetcher();
        let result = ingest::catalog(&store, &mut fetcher, false);
        assert_eq!(result.is_ok(), succeeds, "{pages:?}: {result:?}");
        if let Ok(catalog) = result {
            assert!(catalog.complete);
            assert_eq!(catalog.law_ids.len(), catalog.reported_count);
            assert!(catalog.law_ids.contains(&"ACT_A".into()));
            assert_eq!(catalog.collected_at, now());
        }
    }
}

fn envelope() -> Value {
    json!({
        "law_info":{"law_id":"TEST_ACT","law_num":"Synthetic act","promulgation_date":"1969-01-01"},
        "revision_info":{
            "law_revision_id":"TEST_ACT_REVISION","law_title":"Synthetic title","updated":"2026-11-05T00:00:00Z",
            "amendment_enforcement_date":"1970-04-01","amendment_enforcement_comment":"Confirmed in fixture",
            "amendment_type":"1","mission":"New","repeal_status":"None","current_revision_status":"CurrentEnforced",
            "amendment_law_id":"AMENDING_ACT"
        },
        "law_full_text":base64::engine::general_purpose::STANDARD.encode(b"<Law Era=\"Showa\" Year=\"44\" PromulgateMonth=\"1\" PromulgateDay=\"1\"><LawBody><MainProvision><Article Num=\"1\"><Sentence>Fixture</Sentence></Article></MainProvision></LawBody></Law>")
    })
}

fn expected_history(
    store: &Store,
    law: &ingest::LawInfo,
    info: &ingest::RevisionInfo,
) -> ingest::HistorySnapshot {
    ingest::HistorySnapshot::from_response(
        store,
        &serde_json::to_vec(&json!({"law_info":law,"revisions":[info]})).unwrap(),
        "fixture:history",
    )
    .unwrap()
}

fn semantic_probes() -> Vec<(&'static str, &'static str, Value, Value)> {
    vec![
        ("law_info", "law_id", json!("OTHER_ACT"), json!("OTHER_ACT")),
        (
            "law_info",
            "law_num",
            json!("Changed original notation"),
            json!(true),
        ),
        (
            "law_info",
            "promulgation_date",
            json!("1969-02-01"),
            json!("1969-02-01"),
        ),
        (
            "revision_info",
            "law_revision_id",
            json!("OTHER_REVISION"),
            json!("OTHER_REVISION"),
        ),
        (
            "revision_info",
            "law_title",
            json!("Changed title"),
            json!("Changed title"),
        ),
        (
            "revision_info",
            "updated",
            json!("2026-11-05T00:00:01Z"),
            json!([1, 0]),
        ),
        (
            "revision_info",
            "amendment_promulgate_date",
            json!("1970-03-01"),
            json!("1970-03-01"),
        ),
        (
            "revision_info",
            "amendment_enforcement_date",
            json!("1970-04-02"),
            json!("1970-04-02"),
        ),
        (
            "revision_info",
            "amendment_enforcement_comment",
            json!("Changed date notation"),
            json!(true),
        ),
        (
            "revision_info",
            "amendment_scheduled_enforcement_date",
            json!("1970-04-01"),
            json!(true),
        ),
        (
            "revision_info",
            "amendment_law_id",
            json!("OTHER_AMENDING_ACT"),
            json!("OTHER_AMENDING_ACT"),
        ),
        ("revision_info", "amendment_type", json!("3"), json!(false)),
        ("revision_info", "mission", json!("Partial"), json!(false)),
        (
            "revision_info",
            "repeal_status",
            json!("Expire"),
            json!("expired"),
        ),
        (
            "revision_info",
            "repeal_date",
            json!("1970-04-03"),
            json!("1970-04-03"),
        ),
        (
            "revision_info",
            "current_revision_status",
            json!("PreviousEnforced"),
            json!(false),
        ),
    ]
}

#[test]
fn independently_acquired_history_and_body_metadata_must_agree_before_saving() {
    for (group, field, replacement, _) in semantic_probes() {
        for resumed in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let store = Store::open(directory.path()).unwrap();
            let mut baseline = envelope();
            match field {
                "amendment_promulgate_date" => baseline[group][field] = json!("1970-02-01"),
                "amendment_scheduled_enforcement_date" => {
                    baseline[group][field] = json!("1970-05-01")
                }
                "repeal_date" => baseline[group][field] = json!("1970-04-02"),
                _ => {}
            }
            let original_receipt = if resumed {
                cache(
                    &store,
                    &format!("{EGOV_API}/law_revisions/TEST_ACT"),
                    &json!({"law_info":baseline["law_info"],"revisions":[baseline["revision_info"].clone()]}),
                );
                cache(
                    &store,
                    &format!("{EGOV_API}/law_data/TEST_ACT_REVISION?law_full_text_format=xml"),
                    &baseline,
                );
                assert_eq!(
                    ingest::sync_law(&store, &mut cached_fetcher(), "TEST_ACT", false)
                        .unwrap()
                        .saved,
                    1
                );
                store
                    .latest_fact::<Value>("receipt:TEST_ACT_REVISION", None)
                    .unwrap()
            } else {
                None
            };
            let later = now() + chrono::Duration::seconds(1);
            let mut history = baseline.clone();
            history[group][field] = replacement.clone();
            let law = history["law_info"]["law_id"].as_str().unwrap();
            let revision = history["revision_info"]["law_revision_id"]
                .as_str()
                .unwrap();
            cache_at(
                &store,
                &format!("{EGOV_API}/law_revisions/{law}"),
                &json!({"law_info":history["law_info"],"revisions":[history["revision_info"].clone()]}),
                later,
            );
            cache(
                &store,
                &format!("{EGOV_API}/law_data/{revision}?law_full_text_format=xml"),
                &baseline,
            );
            let mut fetcher = Fetcher::with_clock(Duration::ZERO, Some(0), move || later).unwrap();
            let report = ingest::sync_law(&store, &mut fetcher, law, false).unwrap();
            if field == "updated" {
                assert_eq!((report.saved, report.reused), (1, 0));
                assert!(report.diagnostics.is_empty());
                let receipt = store
                    .latest_fact::<Value>(&format!("receipt:{revision}"), None)
                    .unwrap()
                    .unwrap();
                assert_ne!(Some(&receipt), original_receipt.as_ref());
                let repeated = ingest::sync_law(&store, &mut fetcher, law, false).unwrap();
                assert_eq!((repeated.saved, repeated.reused), (0, 1));
                continue;
            }
            assert_eq!(
                report.saved, 0,
                "{group}.{field}: mismatched source metadata must not publish a revision"
            );
            assert_eq!(report.reused, 0);
            assert_eq!(report.diagnostics.len(), 1);
            assert_eq!(report.diagnostics[0].code, DiagnosticCode::MissingSource);
            assert_eq!(
                store.history("TEST_ACT", None).unwrap().len(),
                usize::from(resumed)
            );
            let unchanged_receipt = if revision == "TEST_ACT_REVISION" {
                original_receipt
            } else {
                None
            };
            assert_eq!(
                store
                    .latest_fact::<Value>(&format!("receipt:{revision}"), None)
                    .unwrap(),
                unchanged_receipt
            );
            let inventory: ingest::LawInventory = store
                .latest_fact(&format!("inventory:{law}"), None)
                .unwrap()
                .unwrap();
            assert!(inventory.current_version.is_none());
        }
    }
}

#[test]
fn history_snapshots_validate_all_subjects_and_keep_missing_revision_slots_absent() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let data = envelope();
    let complete = json!({"law_info":data["law_info"],"revisions":[data["revision_info"].clone()]});
    let snapshot = ingest::HistorySnapshot::from_response(
        &store,
        &serde_json::to_vec(&complete).unwrap(),
        "fixture:history",
    )
    .unwrap();
    assert!(snapshot.revision(0).is_some());
    assert!(snapshot.revision(1).is_none());
    assert!(snapshot.revision(usize::MAX).is_none());
    for field in ["law_id", "law_revision_id"] {
        let mut invalid = complete.clone();
        if field == "law_id" {
            invalid["law_info"][field] = json!("invalid/subject");
        } else {
            invalid["revisions"][0][field] = json!("invalid/subject");
        }
        assert!(
            ingest::HistorySnapshot::from_response(
                &store,
                &serde_json::to_vec(&invalid).unwrap(),
                "fixture:invalid-subject"
            )
            .is_err()
        );
    }
}

#[test]
fn official_complementary_publication_metadata_preserves_and_verifies_both_sources() {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/egov-complementary-publication.json"
    ))
    .unwrap();
    let history_bytes = law_history::codec::decode(
        include_bytes!("../../../fixtures/egov-complementary-history.json.zst"),
        1_000_000,
    )
    .unwrap();
    let body_bytes = law_history::codec::decode(
        include_bytes!("../../../fixtures/egov-complementary-body.json.zst"),
        1_000_000,
    )
    .unwrap();
    assert_eq!(
        law_history::store::sha256(&history_bytes),
        fixture["history_sha256"]
    );
    assert_eq!(
        law_history::store::sha256(&body_bytes),
        fixture["body_sha256"]
    );
    let history: Value = serde_json::from_slice(&history_bytes).unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(
        history["revisions"][0]["amendment_promulgate_date"],
        fixture["publication_date"]
    );
    assert!(body["revision_info"]["amendment_promulgate_date"].is_null());
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    cache_bytes_at(
        &store,
        fixture["history_url"].as_str().unwrap(),
        &history_bytes,
        now(),
    );
    cache_bytes_at(
        &store,
        fixture["body_url"].as_str().unwrap(),
        &body_bytes,
        now(),
    );
    let law = fixture["law_id"].as_str().unwrap();
    let report = ingest::sync_law(&store, &mut cached_fetcher(), law, false).unwrap();
    assert_eq!(report.saved, 1);
    assert!(report.diagnostics.is_empty());
    let selected = law_history::service::select(
        &store,
        law,
        &Query::default(),
        "2026-11-05".parse().unwrap(),
    )
    .unwrap()
    .selected;
    assert_eq!(
        selected
            .revision
            .amendment_promulgated
            .as_ref()
            .unwrap()
            .day()
            .unwrap()
            .to_string(),
        fixture["publication_date"]
    );
    assert_eq!(selected.revision.source.raw_sha256, fixture["body_sha256"]);
    let metadata = selected.revision.metadata_source.as_ref().unwrap();
    assert_eq!(metadata.raw_sha256, fixture["history_sha256"]);
    assert_eq!(metadata.locator, "/law_info, /revisions/0");
    assert_eq!(store.source(&metadata.raw_sha256).unwrap(), history_bytes);
    assert_eq!(
        store.source(&selected.revision.source.raw_sha256).unwrap(),
        body_bytes
    );
    store.rebuild().unwrap();
    assert_eq!(store.version(&selected.version).unwrap(), selected.revision);
    let mut invalid = selected.revision.clone();
    invalid.metadata_source.as_mut().unwrap().raw_sha256 = "F".repeat(64);
    assert!(invalid.validate().is_err());
    let records = std::fs::read_dir(store.root().join("records"))
        .unwrap()
        .count();
    std::fs::write(
        store
            .root()
            .join("sources")
            .join(format!("{}.zst", metadata.raw_sha256)),
        b"corrupt metadata source",
    )
    .unwrap();
    assert!(store.version(&selected.version).is_err());
    assert!(store.save_revision(&selected.revision, now()).is_err());
    assert!(
        store
            .save_revisions(std::slice::from_ref(&selected.revision), now())
            .is_err()
    );
    assert!(store.rebuild().is_err());
    assert_eq!(
        std::fs::read_dir(store.root().join("records"))
            .unwrap()
            .count(),
        records
    );
}

#[test]
fn law_publication_evidence_can_come_from_either_response_without_losing_its_sources() {
    for missing_group in ["body", "history"] {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let mut body = envelope();
        let mut history = envelope();
        if missing_group == "body" {
            body["law_info"]["promulgation_date"] = Value::Null;
        } else {
            history["law_info"]["promulgation_date"] = Value::Null;
        }
        cache(
            &store,
            &format!("{EGOV_API}/law_revisions/TEST_ACT"),
            &json!({"law_info":history["law_info"],"revisions":[history["revision_info"].clone()]}),
        );
        cache(
            &store,
            &format!("{EGOV_API}/law_data/TEST_ACT_REVISION?law_full_text_format=xml"),
            &body,
        );
        let report = ingest::sync_law(&store, &mut cached_fetcher(), "TEST_ACT", false).unwrap();
        assert_eq!(report.saved, 1);
        assert!(report.diagnostics.is_empty());
        let revision = store.history("TEST_ACT", None).unwrap().remove(0).revision;
        assert_eq!(
            revision.promulgated.day().unwrap().to_string(),
            "1969-01-01"
        );
        assert_eq!(revision.sources().count(), 2);
        for source in revision.sources() {
            assert!(store.source(&source.raw_sha256).is_ok());
        }
    }
}

#[test]
fn complementary_optional_metadata_preserves_every_provided_field_across_all_partitions() {
    let mut complete = envelope()["revision_info"].clone();
    for (group, field, value, _) in semantic_probes() {
        if group == "revision_info" {
            complete[field] = value;
        }
    }
    let expected: ingest::RevisionInfo = serde_json::from_value(complete.clone()).unwrap();
    let fields: Vec<_> = source_fields::<ingest::RevisionMetadata>()
        .into_iter()
        .filter(|field| {
            let mut absent = complete.clone();
            absent[field] = Value::Null;
            serde_json::from_value::<ingest::RevisionInfo>(absent).is_ok()
        })
        .collect();
    assert!(!fields.is_empty());
    for field in &fields {
        assert!(
            !complete[field].is_null(),
            "{field} needs a provided-value probe"
        );
    }
    for style in [Value::Null, json!(""), json!(" ")] {
        for partition in 0..(1usize << fields.len()) {
            let mut body = complete.clone();
            let mut history = complete.clone();
            for (index, field) in fields.iter().enumerate() {
                if partition & (1 << index) == 0 {
                    body[field] = style.clone();
                } else {
                    history[field] = style.clone();
                }
            }
            let body: ingest::RevisionInfo = serde_json::from_value(body).unwrap();
            let history: ingest::RevisionInfo = serde_json::from_value(history).unwrap();
            assert_eq!(
                body.metadata.merged(&history.metadata).unwrap(),
                expected.metadata,
                "{partition} {style}"
            );
            assert_eq!(
                history.metadata.merged(&body.metadata).unwrap(),
                expected.metadata
            );
        }
    }
}

#[test]
fn metadata_agreement_matches_independent_unions_of_date_claims() {
    use std::collections::BTreeSet;
    let baseline: ingest::RevisionInfo =
        serde_json::from_value(envelope()["revision_info"].clone()).unwrap();
    let dates = [
        None,
        Some("2029-06-24"),
        Some("2028-12-11"),
        Some("2028-06-04"),
        Some(""),
    ];
    for left_actual in dates {
        for left_scheduled in dates {
            for right_actual in dates {
                for right_scheduled in dates {
                    let mut left = baseline.metadata.clone();
                    left.amendment_enforcement_date = left_actual.map(str::to_owned);
                    left.amendment_scheduled_enforcement_date = left_scheduled.map(str::to_owned);
                    let mut right = baseline.metadata.clone();
                    right.amendment_enforcement_date = right_actual.map(str::to_owned);
                    right.amendment_scheduled_enforcement_date = right_scheduled.map(str::to_owned);
                    let schedules: BTreeSet<_> = [left_scheduled, right_scheduled]
                        .into_iter()
                        .flatten()
                        .filter(|value| !value.is_empty())
                        .collect();
                    let proposed = schedules.first().copied();
                    let actuals: BTreeSet<_> = [left_actual, right_actual]
                        .into_iter()
                        .flatten()
                        .filter(|value| !value.is_empty() && Some(*value) != proposed)
                        .collect();
                    let compatible = schedules.len() <= 1 && actuals.len() <= 1;
                    assert_eq!(
                        left.agrees_with(&right),
                        compatible,
                        "{left_actual:?} {left_scheduled:?} / {right_actual:?} {right_scheduled:?}"
                    );
                    assert_eq!(right.agrees_with(&left), compatible);
                }
            }
        }
    }
}

#[test]
fn official_scheduled_date_placeholders_agree_without_becoming_confirmed_enforcement() {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/egov-scheduled-enforcement.json"
    ))
    .unwrap();
    let history_bytes = law_history::codec::decode(
        include_bytes!("../../../fixtures/egov-health-history.json.zst"),
        1_000_000,
    )
    .unwrap();
    let body_bytes = law_history::codec::decode(
        include_bytes!("../../../fixtures/egov-health-scheduled.json.zst"),
        4_000_000,
    )
    .unwrap();
    assert_eq!(
        law_history::store::sha256(&history_bytes),
        fixture["history_sha256"]
    );
    assert_eq!(
        law_history::store::sha256(&body_bytes),
        fixture["body_sha256"]
    );
    let history: Value = serde_json::from_slice(&history_bytes).unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    let entry = history["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["law_revision_id"] == fixture["revision_id"])
        .unwrap();
    assert_eq!(
        entry["amendment_enforcement_date"],
        fixture["scheduled_date"]
    );
    assert!(body["revision_info"]["amendment_enforcement_date"].is_null());
    let list: ingest::RevisionInfo = serde_json::from_value(entry.clone()).unwrap();
    let data: ingest::RevisionInfo = serde_json::from_value(body["revision_info"].clone()).unwrap();
    assert_ne!(list.metadata, data.metadata);
    assert!(list.metadata.agrees_with(&data.metadata));
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    cache_bytes_at(
        &store,
        fixture["body_url"].as_str().unwrap(),
        &body_bytes,
        now(),
    );
    let law: ingest::LawInfo = serde_json::from_value(history["law_info"].clone()).unwrap();
    let snapshot = expected_history(&store, &law, &list);
    let revision = cached_fetcher()
        .fetch_revision(
            &store,
            fixture["body_url"].as_str().unwrap(),
            false,
            snapshot.revision(0).unwrap(),
        )
        .unwrap();
    assert!(revision.effective.day().is_none());
    assert_eq!(revision.source.raw_sha256, fixture["body_sha256"]);
    let mut projected_history = body;
    projected_history["revision_info"] = entry.clone();
    let other = ingest::revision_from_response(
        &store,
        &serde_json::to_vec(&projected_history).unwrap(),
        "fixture:scheduled-history",
    )
    .unwrap();
    assert_eq!(revision.effective, other.effective);
}

#[test]
fn official_responses_preserve_distinct_update_stamps_for_the_same_revision_metadata() {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/egov-health-response-clocks.json"
    ))
    .unwrap();
    let history_bytes = law_history::codec::decode(
        include_bytes!("../../../fixtures/egov-health-history.json.zst"),
        1_000_000,
    )
    .unwrap();
    let body_bytes = law_history::codec::decode(
        include_bytes!("../../../fixtures/egov-health-body.json.zst"),
        4_000_000,
    )
    .unwrap();
    assert_eq!(
        law_history::store::sha256(&history_bytes),
        fixture["provenance"]["history_sha256"]
    );
    assert_eq!(
        law_history::store::sha256(&body_bytes),
        fixture["provenance"]["body_sha256"]
    );
    let history: Value = serde_json::from_slice(&history_bytes).unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    let revision_id = fixture["history_revision"]["law_revision_id"]
        .as_str()
        .unwrap();
    let entry = history["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["law_revision_id"] == revision_id)
        .unwrap();
    assert_eq!(entry, &fixture["history_revision"]);
    assert_eq!(body["revision_info"], fixture["body_revision"]);
    assert_eq!(history["law_info"], fixture["law_info"]);
    assert_eq!(body["law_info"], fixture["law_info"]);
    let list: ingest::RevisionInfo = serde_json::from_value(entry.clone()).unwrap();
    let data: ingest::RevisionInfo = serde_json::from_value(body["revision_info"].clone()).unwrap();
    assert_eq!(list.metadata, data.metadata);
    assert_ne!(list.updated, data.updated);
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let law = history["law_info"]["law_id"].as_str().unwrap();
    cache_bytes_at(
        &store,
        fixture["provenance"]["history_url"].as_str().unwrap(),
        &history_bytes,
        now(),
    );
    cache_bytes_at(
        &store,
        fixture["provenance"]["body_url"].as_str().unwrap(),
        &body_bytes,
        now(),
    );
    let report = ingest::sync_law(&store, &mut cached_fetcher(), law, false).unwrap();
    assert_eq!(report.saved, 1);
    let selected = law_history::service::select(
        &store,
        law,
        &Query::default(),
        "2026-11-05".parse().unwrap(),
    )
    .unwrap();
    assert_eq!(selected.selected.revision.revision_id, revision_id);
    assert_eq!(
        selected.selected.revision.source.raw_sha256,
        fixture["provenance"]["body_sha256"]
    );
}

fn source_fields<T: utoipa::ToSchema>() -> std::collections::BTreeSet<String> {
    use std::collections::{BTreeMap, BTreeSet};
    fn collect(
        schema: &Value,
        definitions: &BTreeMap<String, Value>,
        active: &mut BTreeSet<String>,
    ) -> BTreeSet<String> {
        let mut fields = BTreeSet::new();
        if let Some(reference) = schema["$ref"].as_str() {
            let name = reference.strip_prefix("#/components/schemas/").unwrap();
            assert!(
                active.insert(name.into()),
                "Recursive source schema needs an explicit field inventory"
            );
            fields.extend(collect(&definitions[name], definitions, active));
            active.remove(name);
        }
        if let Some(properties) = schema["properties"].as_object() {
            fields.extend(properties.keys().cloned());
        }
        if let Some(parts) = schema["allOf"].as_array() {
            for part in parts {
                fields.extend(collect(part, definitions, active));
            }
        }
        fields
    }
    let mut dependencies = Vec::new();
    T::schemas(&mut dependencies);
    let definitions = dependencies
        .into_iter()
        .map(|(name, schema)| (name, serde_json::to_value(schema).unwrap()))
        .collect();
    let fields = collect(
        &serde_json::to_value(T::schema()).unwrap(),
        &definitions,
        &mut BTreeSet::new(),
    );
    assert!(
        !fields.is_empty(),
        "A source schema must expose nonempty field coverage"
    );
    fields
}

#[test]
fn every_typed_source_field_has_a_distinguishing_semantic_probe() {
    use std::collections::BTreeSet;
    let probes = semantic_probes();
    for (name, fields) in [
        ("law_info", source_fields::<ingest::LawInfo>()),
        ("revision_info", source_fields::<ingest::RevisionInfo>()),
    ] {
        let roles: BTreeSet<_> = probes
            .iter()
            .filter(|probe| probe.0 == name)
            .map(|probe| probe.1.to_owned())
            .collect();
        assert_eq!(
            fields, roles,
            "Every typed {name} field needs a semantic probe"
        );
        assert_eq!(
            roles.len(),
            probes.iter().filter(|probe| probe.0 == name).count(),
            "A semantic role must not repeat a field"
        );
    }
    for (group, field, replacement, expected) in probes {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let mut before = envelope();
        if field == "repeal_date" {
            before["revision_info"]["repeal_status"] = json!("Repeal");
            before["revision_info"]["repeal_date"] = json!("1970-04-02");
        }
        let mut after = before.clone();
        after[group][field] = replacement;
        if field == "updated" {
            let list_url = format!("{EGOV_API}/law_revisions/TEST_ACT");
            let data_url =
                format!("{EGOV_API}/law_data/TEST_ACT_REVISION?law_full_text_format=xml");
            let clock = std::sync::Arc::new(std::sync::Mutex::new(now()));
            let current = clock.clone();
            let mut fetcher =
                Fetcher::with_clock(Duration::ZERO, Some(0), move || *current.lock().unwrap())
                    .unwrap();
            for data in [&before, &after] {
                let time = *clock.lock().unwrap();
                cache_at(
                    &store,
                    &list_url,
                    &json!({"law_info":data["law_info"],"revisions":[data["revision_info"].clone()]}),
                    time,
                );
                cache_at(&store, &data_url, data, time);
                let result = ingest::sync_law(&store, &mut fetcher, "TEST_ACT", false).unwrap();
                assert!(result.diagnostics.is_empty());
                assert_eq!(json!([result.saved, result.reused]), expected, "{field}");
                *clock.lock().unwrap() += chrono::Duration::seconds(1);
            }
        } else if field == "current_revision_status" {
            for (index, data) in [before, after].into_iter().enumerate() {
                let isolated = Store::open(directory.path().join(index.to_string())).unwrap();
                let mut other = data.clone();
                other["revision_info"]["law_revision_id"] = json!("SECOND_REVISION");
                other["revision_info"]["current_revision_status"] = json!("PreviousEnforced");
                cache(
                    &isolated,
                    &format!("{EGOV_API}/law_revisions/TEST_ACT"),
                    &json!({"law_info":data["law_info"],"revisions":[data["revision_info"].clone(),other["revision_info"].clone()]}),
                );
                for record in [&data, &other] {
                    cache(
                        &isolated,
                        &format!(
                            "{EGOV_API}/law_data/{}?law_full_text_format=xml",
                            record["revision_info"]["law_revision_id"].as_str().unwrap()
                        ),
                        record,
                    );
                }
                let report =
                    ingest::sync_law(&isolated, &mut cached_fetcher(), "TEST_ACT", false).unwrap();
                assert_eq!(report.saved, 2);
                assert!(report.diagnostics.is_empty());
                let selected = law_history::service::select(
                    &isolated,
                    "TEST_ACT",
                    &Query::default(),
                    "2026-11-05".parse().unwrap(),
                );
                if index == 0 {
                    assert_eq!(
                        selected.unwrap().selected.revision.revision_id,
                        "TEST_ACT_REVISION"
                    );
                } else {
                    assert_eq!(
                        json!(matches!(selected, Err(HistoryError::Ambiguous(_)))),
                        json!(true)
                    );
                    assert_eq!(json!(selected.is_ok()), expected);
                }
            }
        } else {
            let project = |data: &Value| {
                let revision = ingest::revision_from_response(
                    &store,
                    &serde_json::to_vec(data).unwrap(),
                    "fixture:semantic-probe",
                )
                .unwrap();
                match field {
                    "law_id" => json!(revision.law_id),
                    "law_num" => json!(
                        serde_json::to_value(&revision.promulgated).unwrap()["original"]
                            .as_str()
                            .unwrap()
                            .contains("Changed original notation")
                    ),
                    "promulgation_date" => json!(revision.promulgated.day().unwrap().to_string()),
                    "law_revision_id" => json!(revision.revision_id),
                    "law_title" => json!(revision.title),
                    "amendment_promulgate_date" => json!(
                        revision
                            .amendment_promulgated
                            .and_then(|date| date.day())
                            .map(|date| date.to_string())
                    ),
                    "amendment_enforcement_date" => {
                        json!(revision.effective.day().unwrap().to_string())
                    }
                    "amendment_enforcement_comment" => json!(
                        serde_json::to_value(&revision.effective).unwrap()["original"]
                            == "Changed date notation"
                    ),
                    "amendment_scheduled_enforcement_date" => {
                        json!(revision.effective.day().is_none())
                    }
                    "amendment_law_id" => json!(revision.amendment_law_id),
                    "amendment_type" | "mission" => json!(revision.original_enactment),
                    "repeal_status" => json!(revision.termination.map(|value| value.status)),
                    "repeal_date" => json!(revision.repeal.unwrap().to_string()),
                    _ => panic!("Missing semantic projection for {field}"),
                }
            };
            let original = project(&before);
            let changed = project(&after);
            assert_ne!(
                original, changed,
                "{group}.{field} must change its semantic projection"
            );
            assert_eq!(changed, expected, "{group}.{field}");
        }
    }
}

#[test]
fn inventory_current_marker_requires_one_distinct_current_revision() {
    for statuses in [
        ["CurrentEnforced", "PreviousEnforced"],
        ["PreviousEnforced", "CurrentEnforced"],
        ["CurrentEnforced", "CurrentEnforced"],
        ["PreviousEnforced", "UnEnforced"],
        ["Repeal", "UnknownFutureStatus"],
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let mut entries = Vec::new();
        for (index, status) in statuses.iter().enumerate() {
            let mut data = envelope();
            data["revision_info"]["law_revision_id"] = json!(format!("REVISION_{index}"));
            data["revision_info"]["current_revision_status"] = json!(status);
            cache(
                &store,
                &format!("{EGOV_API}/law_data/REVISION_{index}?law_full_text_format=xml"),
                &data,
            );
            entries.push(data["revision_info"].clone());
        }
        cache(
            &store,
            &format!("{EGOV_API}/law_revisions/TEST_ACT"),
            &json!({"law_info":envelope()["law_info"],"revisions":entries}),
        );
        let report = ingest::sync_law(&store, &mut cached_fetcher(), "TEST_ACT", false).unwrap();
        assert!(report.diagnostics.is_empty());
        let inventory: ingest::LawInventory = store
            .latest_fact("inventory:TEST_ACT", None)
            .unwrap()
            .unwrap();
        let expected = match statuses {
            ["CurrentEnforced", "PreviousEnforced"] => Some("REVISION_0"),
            ["PreviousEnforced", "CurrentEnforced"] => Some("REVISION_1"),
            _ => None,
        };
        assert_eq!(inventory.current_revision_id.as_deref(), expected);
        assert_eq!(inventory.current_snapshot_at, Some(now()));
        assert_eq!(inventory.available_revisions, 2);
    }
}

#[test]
fn future_amendment_publication_does_not_make_past_selections_uncertain() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let original = ingest::revision_from_response(
        &store,
        &serde_json::to_vec(&envelope()).unwrap(),
        "https://example.invalid/base",
    )
    .unwrap();
    store.save_revision(&original, now()).unwrap();
    let mut data = envelope();
    data["revision_info"]["law_revision_id"] = json!("PENDING_AMENDMENT");
    data["revision_info"]["amendment_type"] = json!("3");
    data["revision_info"]["amendment_enforcement_date"] = Value::Null;
    data["revision_info"]["amendment_promulgate_date"] = json!("2026-04-01");
    let pending = ingest::revision_from_response(
        &store,
        &serde_json::to_vec(&data).unwrap(),
        "https://example.invalid/pending",
    )
    .unwrap();
    assert_eq!(
        pending.amendment_promulgated.as_ref().unwrap().day(),
        Some("2026-04-01".parse().unwrap())
    );
    assert_eq!(
        pending.promulgated.day(),
        Some("1969-01-01".parse().unwrap())
    );
    store.save_revision(&pending, now()).unwrap();
    let today = "2026-11-05".parse().unwrap();
    for at in ["1970-04-01", "2026-03-31"] {
        let selected = law_history::service::select(
            &store,
            "TEST_ACT",
            &Query {
                at: Some(at.parse().unwrap()),
                ..Default::default()
            },
            today,
        )
        .unwrap();
        assert!(selected.diagnostics.is_empty());
    }
    let selected = law_history::service::select(
        &store,
        "TEST_ACT",
        &Query {
            at: Some("2026-04-01".parse().unwrap()),
            ..Default::default()
        },
        today,
    )
    .unwrap();
    assert_eq!(selected.diagnostics[0].code, DiagnosticCode::UnknownDate);
}

#[test]
fn repeal_without_a_confirmed_calendar_date_never_selects_an_active_revision() {
    for (status, amendment_type, raw_date) in [
        ("Repeal", "3", None),
        ("None", "8", None),
        ("Repeal", "3", Some("invalid date")),
        ("Repeal", "3", Some("1872-12-31")),
        ("None", "3", Some("invalid date")),
        ("None", "3", Some("1872-12-31")),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let mut data = envelope();
        data["revision_info"]["repeal_status"] = json!(status);
        data["revision_info"]["amendment_type"] = json!(amendment_type);
        data["revision_info"]["repeal_date"] = json!(raw_date);
        let revision = ingest::revision_from_response(
            &store,
            &serde_json::to_vec(&data).unwrap(),
            "fixture:undated-repeal",
        )
        .unwrap();
        store.save_revision(&revision, now()).unwrap();
        let selected = law_history::service::select(
            &store,
            "TEST_ACT",
            &Query::default(),
            "1970-04-01".parse().unwrap(),
        );
        assert!(
            matches!(selected, Err(HistoryError::Ambiguous(_))),
            "{status} {amendment_type} {raw_date:?} must not select an active revision"
        );
        assert!(revision.repeal.is_none());
        assert!(revision.termination.as_ref().unwrap().date.day().is_none());
    }
}

#[test]
fn source_termination_dates_keep_missing_uncertain_and_known_evidence_distinct() {
    #[derive(Clone, Copy)]
    enum DateMeaning {
        Missing,
        Uncertain,
        Ended,
        FutureEnd,
    }
    for status in ["None", "Repeal", "Expire", "Suspend", "LossOfEffectiveness"] {
        for amendment_type in ["3", "8"] {
            for (raw, meaning) in [
                (None, DateMeaning::Missing),
                (Some(""), DateMeaning::Missing),
                (Some(" "), DateMeaning::Missing),
                (Some("　"), DateMeaning::Missing),
                (Some("invalid date"), DateMeaning::Uncertain),
                (Some("1872-12-31"), DateMeaning::Uncertain),
                (Some("1970-03-31"), DateMeaning::Ended),
                (Some("1970-04-01"), DateMeaning::Ended),
                (Some("1970-04-02"), DateMeaning::FutureEnd),
            ] {
                let directory = tempfile::tempdir().unwrap();
                let store = Store::open(directory.path()).unwrap();
                let mut data = envelope();
                data["revision_info"]["amendment_enforcement_date"] = json!("1970-01-01");
                data["revision_info"]["amendment_type"] = json!(amendment_type);
                data["revision_info"]["repeal_status"] = json!(status);
                data["revision_info"]["repeal_date"] = json!(raw);
                cache(
                    &store,
                    &format!("{EGOV_API}/law_revisions/TEST_ACT"),
                    &json!({"law_info":data["law_info"],"revisions":[data["revision_info"].clone()]}),
                );
                cache(
                    &store,
                    &format!("{EGOV_API}/law_data/TEST_ACT_REVISION?law_full_text_format=xml"),
                    &data,
                );
                let report =
                    ingest::sync_law(&store, &mut cached_fetcher(), "TEST_ACT", false).unwrap();
                assert_eq!(
                    report.saved, 1,
                    "{status} {amendment_type} {raw:?}: {:?}",
                    report.diagnostics
                );
                assert!(report.diagnostics.is_empty());
                let stored = store.history("TEST_ACT", None).unwrap().remove(0);
                let processing = matches!(status, "Expire" | "Suspend" | "LossOfEffectiveness");
                let uncertain = processing
                    || matches!(meaning, DateMeaning::Uncertain)
                    || (matches!(meaning, DateMeaning::Missing)
                        && (status == "Repeal" || amendment_type == "8"));
                if uncertain {
                    assert!(stored.revision.repeal.is_none());
                    assert!(
                        stored
                            .revision
                            .termination
                            .as_ref()
                            .unwrap()
                            .date
                            .day()
                            .is_none()
                    );
                    if !processing && matches!(meaning, DateMeaning::Uncertain) {
                        let evidence = serde_json::to_value(
                            &stored.revision.termination.as_ref().unwrap().date,
                        )
                        .unwrap();
                        assert_eq!(evidence["original"], json!(raw.unwrap()));
                    }
                } else if matches!(meaning, DateMeaning::Ended | DateMeaning::FutureEnd) {
                    assert_eq!(stored.revision.repeal.unwrap().to_string(), raw.unwrap());
                } else {
                    assert!(stored.revision.repeal.is_none());
                    assert!(stored.revision.termination.is_none());
                }
                for rebuild in [false, true] {
                    if rebuild {
                        store.rebuild().unwrap();
                    }
                    let selected = law_history::service::select(
                        &store,
                        "TEST_ACT",
                        &Query::default(),
                        "1970-04-01".parse().unwrap(),
                    );
                    if uncertain {
                        assert!(
                            matches!(selected, Err(HistoryError::Ambiguous(_))),
                            "{status} {amendment_type} {raw:?}"
                        );
                    } else if matches!(meaning, DateMeaning::Ended) {
                        assert!(
                            matches!(selected, Err(HistoryError::NotFound(_))),
                            "{status} {amendment_type} {raw:?}"
                        );
                    } else {
                        assert_eq!(selected.unwrap().selected.version, stored.version);
                    }
                    let pinned = law_history::service::select(
                        &store,
                        "TEST_ACT",
                        &Query {
                            version: Some(stored.version.clone()),
                            ..Default::default()
                        },
                        "1970-04-01".parse().unwrap(),
                    )
                    .unwrap();
                    assert_eq!(
                        pinned
                            .diagnostics
                            .iter()
                            .any(|item| item.code == DiagnosticCode::UnknownDate),
                        uncertain,
                        "{status} {amendment_type} {raw:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn unknown_repeal_status_values_are_refused_at_the_source_boundary() {
    for status in ["UnknownFutureStatus", "repeal", "", " Repeal", "None "] {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let mut data = envelope();
        data["revision_info"]["repeal_status"] = json!(status);
        assert!(
            ingest::revision_from_response(
                &store,
                &serde_json::to_vec(&data).unwrap(),
                "fixture:unknown-status"
            )
            .is_err(),
            "{status}"
        );
    }
}

#[test]
fn source_metadata_distinguishes_enactment_repeal_and_uncertain_termination() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    for (amendment_type, mission, status, kind, enacted, termination) in [
        ("1", "New", "None", "enactment", true, None),
        ("1", "Partial", "None", "consolidated", false, None),
        ("2", "New", "None", "consolidated", false, None),
        ("8", "Partial", "None", "repeal", false, None),
        ("2", "Partial", "Repeal", "repeal", false, None),
        (
            "2",
            "Partial",
            "Expire",
            "consolidated",
            false,
            Some("expired"),
        ),
        (
            "2",
            "Partial",
            "Suspend",
            "consolidated",
            false,
            Some("suspended"),
        ),
        (
            "2",
            "Partial",
            "LossOfEffectiveness",
            "consolidated",
            false,
            Some("lost_effectiveness"),
        ),
    ] {
        let mut input = envelope();
        input["revision_info"]["amendment_type"] = json!(amendment_type);
        input["revision_info"]["mission"] = json!(mission);
        input["revision_info"]["repeal_status"] = json!(status);
        input["revision_info"]["repeal_date"] = if status == "None" && amendment_type != "8" {
            Value::Null
        } else {
            json!("1970-04-02")
        };
        let bytes = serde_json::to_vec(&input).unwrap();
        let revision = ingest::revision_from_response(&store, &bytes, "fixture:source").unwrap();
        assert_eq!(serde_json::to_value(&revision.kind).unwrap(), kind);
        assert_eq!(revision.original_enactment, enacted);
        assert_eq!(
            revision.repeal.is_some(),
            status == "Repeal" || amendment_type == "8"
        );
        assert_eq!(revision.amendment_law_id.as_deref(), Some("AMENDING_ACT"));
        assert_eq!(
            revision.effective.day(),
            Some("1970-04-01".parse().unwrap())
        );
        assert_eq!(revision.source.locator, "/law_full_text");
        assert_eq!(revision.source.confidence, Confidence::Official);
        assert_eq!(store.source(&revision.source.raw_sha256).unwrap(), bytes);
        match (revision.termination, termination) {
            (Some(value), Some(expected)) => {
                assert_eq!(serde_json::to_value(value.status).unwrap(), expected);
                assert!(value.date.day().is_none());
            }
            (None, None) => {}
            _ => panic!("Termination changed for {status}"),
        }
    }
    for field in [
        "amendment_scheduled_enforcement_date",
        "amendment_enforcement_date",
    ] {
        let mut input = envelope();
        input["revision_info"][field] = json!("1872-12-31");
        input["revision_info"]["amendment_law_id"] = json!("");
        let revision = ingest::revision_from_response(
            &store,
            &serde_json::to_vec(&input).unwrap(),
            "fixture:source",
        )
        .unwrap();
        assert!(revision.effective.day().is_none());
        assert!(revision.amendment_law_id.is_none());
    }
}

#[test]
fn calendar_text_requires_exact_ascii_components_and_separators() {
    let reference = b"1970-01-01";
    for position in 0..reference.len() {
        for byte in 0..=127 {
            let mut bytes = *reference;
            bytes[position] = byte;
            let value = std::str::from_utf8(&bytes).unwrap();
            let accepted = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .is_ok_and(|date| date.format("%Y-%m-%d").to_string() == value);
            assert_eq!(value.parse::<Day>().is_ok(), accepted, "{value:?}");
        }
    }
    for value in [
        "1970-01-1",
        "1970-001-01",
        "1970/01/01",
        "1970-01+01",
        " 1970-01-01",
        "1970-01-01 ",
    ] {
        assert!(value.parse::<Day>().is_err());
    }
}

#[test]
fn revision_validation_rejects_each_invalid_field_and_preserves_equal_repeal_dates() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    assert_eq!(
        ingest::import_bundle(&store, include_bytes!("../../../fixtures/demo.json"))
            .unwrap()
            .saved,
        2
    );
    let revision = ingest::revision_from_response(
        &store,
        &serde_json::to_vec(&envelope()).unwrap(),
        "fixture",
    )
    .unwrap();
    assert!(revision.validate().is_ok());
    let mut invalid = Vec::new();
    let mut item = revision.clone();
    item.schema_version = 0;
    invalid.push(item);
    let mut item = revision.clone();
    item.title = " ".into();
    invalid.push(item);
    let mut item = revision.clone();
    item.law_id = "invalid/path".into();
    invalid.push(item);
    let mut item = revision.clone();
    item.revision_id.clear();
    invalid.push(item);
    for digest in [
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        let mut item = revision.clone();
        item.source.raw_sha256 = digest;
        invalid.push(item);
    }
    let mut item = revision.clone();
    item.tree.tag = "Other".into();
    invalid.push(item);
    let mut item = revision.clone();
    item.repeal = Some("1970-03-31".parse().unwrap());
    invalid.push(item);
    for item in invalid {
        assert!(item.validate().is_err(), "{item:?}");
    }
    for end in ["1970-04-01", "1970-04-02"] {
        let mut item = revision.clone();
        item.repeal = Some(end.parse().unwrap());
        assert!(item.validate().is_ok());
    }
    for byte in 0..=127u8 {
        let value = String::from_utf8(vec![byte]).unwrap();
        let expected = byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-');
        assert_eq!(validate_id(&value).is_ok(), expected, "{value:?}");
    }
    assert!(validate_id("").is_err());
    assert!(validate_id(&"x".repeat(200)).is_ok());
    assert!(validate_id(&"x".repeat(201)).is_err());
}

#[test]
fn bill_import_checks_identity_and_links_only_sourced_laws_and_speeches() {
    use law_history::parliament::{self, Bill, BillId, BillKind, House, SpeechLink};
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let source = Provenance {
        url: "fixture:bill".into(),
        raw_sha256: store.preserve(b"bill evidence").unwrap(),
        media_type: "application/json".into(),
        attribution: "Synthetic fixture".into(),
        terms_url: "fixture:terms".into(),
        confidence: Confidence::Synthetic,
        locator: "/bill".into(),
    };
    let bill = Bill {
        schema_version: SCHEMA_VERSION,
        id: BillId {
            house: House::Representatives,
            kind: BillKind::Cabinet,
            session: 1,
            number: 1,
        },
        title: "Synthetic bill".into(),
        enacted_law_ids: vec!["TEST_ACT".into()],
        submitters: vec!["Fixture submitter".into()],
        reason: "Fixture reason".into(),
        speeches: vec![SpeechLink {
            speech_id: "SPEECH".into(),
            confidence: Confidence::Synthetic,
            basis: "Source linkage fixture".into(),
        }],
        source,
    };
    let imported =
        parliament::import_bill(&store, &serde_json::to_vec(&bill).unwrap(), now()).unwrap();
    assert_eq!(imported.id.key(), "representatives-1-cabinet-1");
    assert_eq!(
        law_history::service::parliament(&store, "TEST_ACT", None)
            .unwrap()
            .bills
            .len(),
        1
    );
    assert!(
        law_history::service::parliament(&store, "OTHER_ACT", None)
            .unwrap()
            .bills
            .is_empty()
    );
    assert!(
        law_history::service::parliament(
            &store,
            "TEST_ACT",
            Some(now() - chrono::Duration::days(1))
        )
        .unwrap()
        .bills
        .is_empty()
    );
    let mut invalid = Vec::new();
    let mut item = bill.clone();
    item.schema_version = 0;
    invalid.push(item);
    let mut item = bill.clone();
    item.id.session = 0;
    invalid.push(item);
    let mut item = bill.clone();
    item.id.number = 0;
    invalid.push(item);
    let mut item = bill.clone();
    item.title = " ".into();
    invalid.push(item);
    let mut item = bill.clone();
    item.enacted_law_ids = vec!["invalid/path".into()];
    invalid.push(item);
    let mut item = bill;
    item.source.raw_sha256 = "a".repeat(64);
    invalid.push(item);
    for item in invalid {
        assert!(
            parliament::import_bill(&store, &serde_json::to_vec(&item).unwrap(), now()).is_err()
        );
    }
}

fn speech_url(session: u32, query: &str, start: usize) -> String {
    let mut url = reqwest::Url::parse("https://kokkai.ndl.go.jp/api/speech").unwrap();
    url.query_pairs_mut()
        .append_pair("recordPacking", "json")
        .append_pair("maximumRecords", "100")
        .append_pair("sessionFrom", &session.to_string())
        .append_pair("sessionTo", &session.to_string())
        .append_pair("any", query)
        .append_pair("startRecord", &start.to_string());
    url.into()
}

fn speech_record(id: &str) -> Value {
    json!({"speechID":id,"issueID":"MEETING","session":217,"nameOfHouse":"House","nameOfMeeting":"Committee","date":"1969-12-31","speaker":"Fixture speaker","speech":"Fixture speech","speechURL":"https://example.invalid/speech"})
}

#[test]
fn parliamentary_collection_checks_input_pagination_counts_and_preserved_sources() {
    use law_history::parliament::{self, Speech};
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let first = json!({"numberOfRecords":3,"numberOfReturn":2,"startRecord":1,"nextRecordPosition":3,"speechRecord":[speech_record("SP1"),speech_record("SP2")]});
    let last = json!({"numberOfRecords":3,"numberOfReturn":1,"startRecord":3,"nextRecordPosition":0,"speechRecord":[speech_record("SP3")]});
    cache(&store, &speech_url(217, "fixture", 1), &first);
    cache(&store, &speech_url(217, "fixture", 3), &last);
    let mut fetcher = cached_fetcher();
    assert_eq!(
        parliament::sync_speeches(&store, &mut fetcher, 217, "fixture", 2).unwrap(),
        3
    );
    let speech: Speech = store.latest_fact("speech:SP3", None).unwrap().unwrap();
    assert_eq!(speech.speaker, "Fixture speaker");
    assert_eq!(speech.text, "Fixture speech");
    assert_eq!(speech.url, "https://example.invalid/speech");
    assert_eq!(
        store.source(&speech.source.raw_sha256).unwrap(),
        serde_json::to_vec(&last).unwrap()
    );
    assert_eq!(speech.source.locator, "/speechRecord");
    for (session, query, pages) in [(0, "fixture", 1), (217, "", 1), (217, "fixture", 0)] {
        let error =
            parliament::sync_speeches(&store, &mut fetcher, session, query, pages).unwrap_err();
        assert!(error.to_string().contains("positive page bound"));
    }
    for next in [Some(1), Some(0), None] {
        let page = json!({"numberOfRecords":1,"numberOfReturn":1,"startRecord":1,"nextRecordPosition":next,"speechRecord":[speech_record("SINGLE")]});
        let query = format!("cursor-{next:?}");
        cache(&store, &speech_url(217, &query, 1), &page);
        let result = parliament::sync_speeches(&store, &mut fetcher, 217, &query, 1);
        if next == Some(1) {
            assert!(result.unwrap_err().to_string().contains("non-advancing"));
        } else {
            assert_eq!(result.unwrap(), 1);
        }
    }
}

#[tokio::test]
async fn revision_sources_revalidate_metadata_conflicts_once_without_retrying_other_failures() {
    use axum::{Json, Router, extract::Path, routing::get};
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };
    let calls = Arc::new(Mutex::new(BTreeMap::<String, usize>::new()));
    let counted = calls.clone();
    let clock = Arc::new(Mutex::new(now()));
    let completed = clock.clone();
    let correct = envelope();
    let mut stale = correct.clone();
    stale["revision_info"]["law_title"] = json!("Stale title");
    let mut malformed = correct.clone();
    malformed["law_full_text"] = json!(base64::engine::general_purpose::STANDARD.encode(b"<Law>"));
    let mut wrong_identity = correct.clone();
    wrong_identity["law_info"]["law_id"] = json!("OTHER_ACT");
    let served_stale = stale.clone();
    let served_correct = correct.clone();
    let app = Router::new().route(
        "/{case}",
        get(move |Path(case): Path<String>| {
            let counted = counted.clone();
            let completed = completed.clone();
            let correct = served_correct.clone();
            let stale = served_stale.clone();
            let malformed = malformed.clone();
            let wrong_identity = wrong_identity.clone();
            async move {
                let count = {
                    let mut calls = counted.lock().unwrap();
                    let count = calls.entry(case.clone()).or_default();
                    *count += 1;
                    *count
                };
                *completed.lock().unwrap() += chrono::Duration::seconds(1);
                Json(match case.as_str() {
                    "cached" => correct,
                    "first_stale" | "unlimited" if count > 1 => correct,
                    "malformed" => malformed,
                    "identity" => wrong_identity,
                    _ => stale,
                })
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::task::spawn_blocking(move || {
        let directory = tempfile::tempdir().unwrap();
        let law: ingest::LawInfo = serde_json::from_value(correct["law_info"].clone()).unwrap();
        let info: ingest::RevisionInfo =
            serde_json::from_value(correct["revision_info"].clone()).unwrap();
        for (case, budget, refresh, cached, succeeds, expected_calls) in [
            ("cached", Some(1), false, true, true, 1),
            ("first_stale", Some(2), false, false, true, 2),
            ("unlimited", None, false, false, true, 2),
            ("budget", Some(1), false, false, false, 1),
            ("exhausted", Some(0), false, true, false, 0),
            ("refresh", Some(2), true, false, false, 1),
            ("conflict", Some(2), false, false, false, 2),
            ("malformed", Some(2), false, false, false, 1),
            ("identity", Some(2), false, false, false, 1),
        ] {
            let store = Store::open(directory.path().join(case)).unwrap();
            let url = format!("http://{address}/{case}");
            if cached {
                cache_at(&store, &url, &stale, *clock.lock().unwrap());
            }
            let current = clock.clone();
            let mut fetcher =
                Fetcher::with_clock(Duration::ZERO, budget, move || *current.lock().unwrap())
                    .unwrap();
            let expected = expected_history(&store, &law, &info);
            let result =
                fetcher.fetch_revision(&store, &url, refresh, expected.revision(0).unwrap());
            assert_eq!(result.is_ok(), succeeds, "{case}: {result:?}");
            assert_eq!(
                calls.lock().unwrap().get(case).copied().unwrap_or(0),
                expected_calls,
                "{case}"
            );
            if let Ok(revision) = result {
                assert_eq!(revision.title, "Synthetic title");
                let source: Value =
                    serde_json::from_slice(&store.source(&revision.source.raw_sha256).unwrap())
                        .unwrap();
                assert_eq!(source["revision_info"], correct["revision_info"]);
            }
        }
    })
    .await
    .unwrap();
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn legacy_snapshots_are_revalidated_only_with_an_available_request_budget() {
    use axum::{Router, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let app = Router::new().route(
        "/snapshot",
        get(move || {
            let counted = counted.clone();
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
                "fresh snapshot"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::task::spawn_blocking(move || {
        let directory = tempfile::tempdir().unwrap();
        let url = format!("http://{address}/snapshot");
        let key = format!("cache:{}", law_history::store::sha256(url.as_bytes()));
        for (index, budget) in [Some(0), Some(1), None].into_iter().enumerate() {
            let store = Store::open(directory.path().join(index.to_string())).unwrap();
            let digest = store.preserve(b"legacy snapshot").unwrap();
            store.fact(&key, &json!({"digest":digest,"collected_at":now()}), now()).unwrap();
            let later = now() + chrono::Duration::seconds(1);
            let mut fetcher = Fetcher::with_clock(Duration::ZERO, budget, move || later).unwrap();
            let response = fetcher.fetch_snapshot(&store, &url, false).unwrap();
            if budget == Some(0) {
                assert_eq!(response.bytes, b"legacy snapshot");
                assert!(response.acquired_at.is_none());
            } else {
                assert_eq!(response.bytes, b"fresh snapshot");
                assert_eq!(response.acquired_at, Some(later));
                let cached = fetcher.fetch_snapshot(&store, &url, false).unwrap();
                assert_eq!(cached.bytes, response.bytes);
                assert_eq!(cached.acquired_at, response.acquired_at);
            }
            assert_eq!(calls.load(Ordering::SeqCst), index);
        }
        let store = Store::open(directory.path().join("invalid-time")).unwrap();
        let digest = store.preserve(b"invalid clock provenance").unwrap();
        store.fact(&key, &json!({"digest":digest,"collected_at":now(),"acquired_at":now()+chrono::Duration::nanoseconds(1)}), now()).unwrap();
        let Err(error) = cached_fetcher().fetch_snapshot(&store, &url, false) else {
            panic!("Snapshots without acquisition evidence must be refused");
        };
        assert!(error.to_string().contains("acquisition time"));
    }).await.unwrap();
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn fetcher_enforces_response_limits_and_reuses_only_same_day_cache() {
    use axum::{Router, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let clock = Arc::new(std::sync::Mutex::new(now()));
    let completed = clock.clone();
    let app = Router::new()
        .route(
            "/small",
            get(move || {
                let counted = counted.clone();
                let completed = completed.clone();
                async move {
                    counted.fetch_add(1, Ordering::SeqCst);
                    *completed.lock().unwrap() += chrono::Duration::seconds(1);
                    "source response"
                }
            }),
        )
        .route("/bounded", get(|| async { vec![b'x'; 67_108_864] }))
        .route("/oversized", get(|| async { vec![b'x'; 67_108_865] }));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    tokio::task::spawn_blocking(move || {
        let store = Store::open(directory.path()).unwrap();
        let current = clock.clone();
        let mut fetcher =
            Fetcher::with_clock(Duration::ZERO, Some(5), move || *current.lock().unwrap()).unwrap();
        let url = format!("http://{address}/small");
        assert_eq!(
            fetcher.fetch(&store, &url, false).unwrap(),
            b"source response"
        );
        assert_eq!(
            fetcher.fetch(&store, &url, false).unwrap(),
            b"source response"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let key = format!("cache:{}", law_history::store::sha256(url.as_bytes()));
        assert!(
            store
                .latest_fact::<Value>(&key, Some(now()))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store.latest_fact::<Value>(&key, None).unwrap().unwrap()["collected_at"],
            json!(now() + chrono::Duration::seconds(1))
        );
        *clock.lock().unwrap() = now();
        assert_eq!(
            fetcher.fetch(&store, &url, false).unwrap(),
            b"source response"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        *clock.lock().unwrap() = now() + chrono::Duration::days(1);
        assert_eq!(
            fetcher.fetch(&store, &url, false).unwrap(),
            b"source response"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            fetcher
                .fetch(&store, &format!("http://{address}/bounded"), false)
                .unwrap()
                .len(),
            67_108_864
        );
        let Err(error) = fetcher.fetch(&store, &format!("http://{address}/oversized"), false)
        else {
            panic!("Oversized source responses must be refused");
        };
        assert!(format!("{error:#}").contains("64 MiB"), "{error:#}");
        let Err(error) = fetcher.fetch(&store, &url, true) else {
            panic!("Exhausted request budgets must be refused");
        };
        assert!(error.to_string().contains("Request bound reached"));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    })
    .await
    .unwrap();
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

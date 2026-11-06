use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn cli(store: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_law-history"))
        .arg("--store")
        .arg(store)
        .arg("--json")
        .args(args)
        .output()
        .unwrap()
}

fn imported() -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let store = directory.path().join("store");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/demo.json");
    let imported = cli(&store, &["sync", "--import", fixture.to_str().unwrap()]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    (directory, store)
}

#[test]
fn cli_search_show_diff_reindex_and_failures_emit_one_json_value() {
    let (_directory, store) = imported();
    let search = cli(&store, &["search", "Demonstration", "--at", "1969-12-31"]);
    assert!(search.status.success());
    let found: Value = serde_json::from_slice(&search.stdout).unwrap();
    assert_eq!(found["hits"][0]["effective"]["date"], "1969-04-01");
    let log: Value = serde_json::from_slice(&cli(&store, &["log", "DEMO_ACT"]).stdout).unwrap();
    let from = log[0]["version"].as_str().unwrap();
    let to = log[1]["version"].as_str().unwrap();
    let diff = cli(&store, &["diff", from, to]);
    assert!(diff.status.success());
    let changes: Value = serde_json::from_slice(&diff.stdout).unwrap();
    assert!(!changes["changes"].as_array().unwrap().is_empty());
    assert!(cli(&store, &["reindex"]).status.success());
    assert!(cli(&store, &["verify"]).status.success());
    let missing = cli(&store, &["show", "MISSING_LAW"]);
    assert!(!missing.status.success());
    let error: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert!(error["error"].is_string());
}

#[test]
fn exported_repository_can_be_cloned_and_uses_generation_time() {
    let (directory, store) = imported();
    let export = directory.path().join("export");
    let started = chrono::Utc::now().timestamp();
    let result = Command::new(env!("CARGO_BIN_EXE_law-history"))
        .arg("--store")
        .arg(&store)
        .arg("--json")
        .args(["export", "git"])
        .arg(&export)
        .args(["--law", "DEMO_ACT", "--at", "1969-12-31"])
        .env("GIT_AUTHOR_DATE", "1970-01-01T00:00:01Z")
        .env("GIT_COMMITTER_DATE", "1970-01-01T00:00:01Z")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let log = Command::new("git")
        .arg("-C")
        .arg(&export)
        .args(["log", "-1", "--format=%at %ct"])
        .output()
        .unwrap();
    assert!(log.status.success());
    for timestamp in String::from_utf8(log.stdout).unwrap().split_whitespace() {
        assert!(timestamp.parse::<i64>().unwrap() >= started);
    }
    let clone = directory.path().join("clone");
    let cloned = Command::new("git")
        .arg("clone")
        .arg(&export)
        .arg(&clone)
        .output()
        .unwrap();
    assert!(cloned.status.success());
    let text = fs::read_to_string(clone.join("DEMO_ACT/text.txt")).unwrap();
    assert!(text.contains("申請は書面で行う。"));
    assert!(clone.join("DEMO_ACT/history.json").is_file());
    let refused = cli(
        &store,
        &[
            "export",
            "git",
            export.to_str().unwrap(),
            "--law",
            "DEMO_ACT",
        ],
    );
    assert!(!refused.status.success());
}

#[test]
fn cached_sync_counts_laws_and_emits_only_one_json_report_on_failure() {
    use law_history::{Store, ingest::EGOV_API};
    use serde_json::json;
    for _ in 0..2 {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let now = chrono::Utc::now();
        let cache = |url: &str, value: Value| {
            let digest = store
                .preserve(&serde_json::to_vec(&value).unwrap())
                .unwrap();
            store
                .fact(
                    &format!("cache:{}", law_history::store::sha256(url.as_bytes())),
                    &json!({"digest":digest,"collected_at":now}),
                    now,
                )
                .unwrap();
        };
        let info = |law: &str| json!({"law_id":law,"law_num":"Fixture act","promulgation_date":"1969-01-01"});
        cache(
            &format!("{EGOV_API}/laws?limit=1000&offset=0&order=%2Blaw_info.law_id"),
            json!({"total_count":2,"next_offset":null,"laws":[{"law_info":info("FIRST_ACT")},{"law_info":info("SECOND_ACT")}]}),
        );
        for law in ["FIRST_ACT", "SECOND_ACT"] {
            let revision = json!({"law_revision_id":format!("{law}_REV"),"law_title":"Fixture act","updated":"2026-11-05T00:00:00Z","amendment_enforcement_date":"1970-01-01","amendment_type":"1","mission":"New","repeal_status":"None","current_revision_status":"CurrentEnforced"});
            cache(
                &format!("{EGOV_API}/law_revisions/{law}"),
                json!({"law_info":info(law),"revisions":[revision.clone()]}),
            );
            cache(
                &format!("{EGOV_API}/law_data/{law}_REV?law_full_text_format=xml"),
                json!({"law_info":info(law),"revision_info":revision,"law_full_text":"PExhdy8+"}),
            );
        }
        let result = cli(directory.path(), &["sync", "--all", "--max-requests", "0"]);
        if chrono::Utc::now().date_naive() != now.date_naive() {
            continue;
        }
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
        let report: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["laws"], 2);
        assert_eq!(report["saved"], 2);
        assert_eq!(report["reused"], 0);
        let repeated = cli(directory.path(), &["sync", "--all", "--max-requests", "0"]);
        assert!(repeated.status.success());
        let repeated: Value = serde_json::from_slice(&repeated.stdout).unwrap();
        assert_eq!(repeated["laws"], 2);
        assert_eq!(repeated["saved"], 0);
        assert_eq!(repeated["reused"], 2);
        let restored_path = directory.path().join("reobserved");
        let restored = cli(
            &restored_path,
            &["import-cache", "--from", directory.path().to_str().unwrap()],
        );
        assert!(
            restored.status.success(),
            "{}",
            String::from_utf8_lossy(&restored.stdout)
        );
        let restored: Value = serde_json::from_slice(&restored.stdout).unwrap();
        assert_eq!(restored["imported"], 5);
        let replay = cli(&restored_path, &["sync", "--all", "--max-requests", "0"]);
        assert!(
            replay.status.success(),
            "{}",
            String::from_utf8_lossy(&replay.stdout)
        );
        let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
        assert_eq!(replay["saved"], 2);
        assert!(
            Store::open(&restored_path)
                .unwrap()
                .history("FIRST_ACT", Some(now))
                .unwrap()
                .is_empty()
        );
        let invalid = cli(directory.path(), &["sync", "--max-requests", "0"]);
        assert!(!invalid.status.success());
        assert!(
            serde_json::from_slice::<Value>(&invalid.stdout).unwrap()["error"]
                .as_str()
                .unwrap()
                .contains("Choose")
        );
        let scoped = cli(
            directory.path(),
            &["sync", "--law", "FIRST_ACT", "--max-requests", "0"],
        );
        assert!(scoped.status.success());
        let catalog = cli(
            directory.path(),
            &["sync", "--catalog-only", "--max-requests", "0"],
        );
        assert!(catalog.status.success());
        let mut revision: Value = serde_json::from_slice(
            &store
                .source(
                    store
                        .latest_fact::<Value>(
                            &format!(
                                "cache:{}",
                                law_history::store::sha256(
                                    format!("{EGOV_API}/law_revisions/FIRST_ACT").as_bytes()
                                )
                            ),
                            None,
                        )
                        .unwrap()
                        .unwrap()["digest"]
                        .as_str()
                        .unwrap(),
                )
                .unwrap(),
        )
        .unwrap();
        revision["revisions"][0]["law_revision_id"] = json!("ABSENT_REVISION");
        let timestamp = chrono::Utc::now();
        let digest = store
            .preserve(&serde_json::to_vec(&revision).unwrap())
            .unwrap();
        store
            .fact(
                &format!(
                    "cache:{}",
                    law_history::store::sha256(
                        format!("{EGOV_API}/law_revisions/FIRST_ACT").as_bytes()
                    )
                ),
                &json!({"digest":digest,"collected_at":timestamp}),
                timestamp,
            )
            .unwrap();
        let failed = cli(
            directory.path(),
            &["sync", "--law", "FIRST_ACT", "--max-requests", "0"],
        );
        assert!(!failed.status.success());
        let report: Value = serde_json::from_slice(&failed.stdout).unwrap();
        assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
        assert!(String::from_utf8_lossy(&failed.stderr).contains("Sync preserved source failures"));
        return;
    }
    panic!("INCONCLUSIVE: the collection day changed during both fixture attempts");
}

#[test]
fn amendment_simulation_and_comparison_keep_success_refusal_and_identity_distinct() {
    use law_history::{Store, amendment::*, model::*, reconstruction::OfficialComparison};
    use std::collections::BTreeMap;
    let (directory, path) = imported();
    let store = Store::open(&path).unwrap();
    let history = store.history("DEMO_ACT", None).unwrap();
    let base = &history[0];
    let mut expected = history[1].revision.clone();
    expected.source.confidence = Confidence::Official;
    let expected_version = store.save_revision(&expected, chrono::Utc::now()).unwrap();
    let sentence = "/Law/LawBody[index=1]/MainProvision[index=1]/Article[num=1]/Paragraph[num=1]/ParagraphSentence[index=1]/Sentence[index=1]";
    let scenario = Scenario {
        schema_version: SCHEMA_VERSION,
        name: "Synthetic CLI transition".into(),
        bases: BTreeMap::from([("DEMO_ACT".into(), base.version.clone())]),
        at: "1970-04-01".parse().unwrap(),
        known_at: None,
        reference_context: None,
        steps: vec![AmendmentStep {
            id: "CHANGE".into(),
            effective: expected.effective.clone(),
            after: Vec::new(),
            operations: vec![SourcedOperation {
                operation: Operation::ReplaceText {
                    law_id: "DEMO_ACT".into(),
                    path: sentence.into(),
                    old: "書面".into(),
                    new: "電磁的記録".into(),
                },
                evidence: base.revision.source.clone(),
            }],
            adjustments: Vec::new(),
        }],
    };
    let scenario_path = directory.path().join("scenario.json");
    fs::write(&scenario_path, serde_json::to_vec(&scenario).unwrap()).unwrap();
    let simulated = cli(&path, &["simulate", scenario_path.to_str().unwrap()]);
    assert!(
        simulated.status.success(),
        "{}",
        String::from_utf8_lossy(&simulated.stderr)
    );
    let result: Value = serde_json::from_slice(&simulated.stdout).unwrap();
    assert!(result["diagnostics"].as_array().unwrap().is_empty());
    let mut refused = scenario.clone();
    let Operation::ReplaceText { old, .. } = &mut refused.steps[0].operations[0].operation else {
        unreachable!()
    };
    *old = "ABSENT ORIGINAL".into();
    fs::write(&scenario_path, serde_json::to_vec(&refused).unwrap()).unwrap();
    let result = cli(&path, &["simulate", scenario_path.to_str().unwrap()]);
    assert!(!result.status.success());
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(!result["diagnostics"].as_array().unwrap().is_empty());
    let amendment_path = directory.path().join("amendment.txt");
    fs::write(
        &amendment_path,
        "第一条中「書面」を「電磁的記録」に改める。",
    )
    .unwrap();
    let parsed = cli(
        &path,
        &[
            "amendment",
            "DEMO_ACT",
            &base.version,
            amendment_path.to_str().unwrap(),
            "--source-url",
            "urn:synthetic:amendment",
        ],
    );
    assert!(parsed.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&parsed.stdout).unwrap()["operations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let wrong_law = cli(
        &path,
        &[
            "amendment",
            "OTHER_ACT",
            &base.version,
            amendment_path.to_str().unwrap(),
            "--source-url",
            "urn:synthetic:amendment",
        ],
    );
    assert!(!wrong_law.status.success());
    assert!(
        serde_json::from_slice::<Value>(&wrong_law.stdout).unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("different law")
    );
    fs::write(&amendment_path, "Unsupported source sentence").unwrap();
    let refused = cli(
        &path,
        &[
            "amendment",
            "DEMO_ACT",
            &base.version,
            amendment_path.to_str().unwrap(),
            "--source-url",
            "urn:synthetic:amendment",
        ],
    );
    assert!(!refused.status.success());
    assert!(
        !serde_json::from_slice::<Value>(&refused.stdout).unwrap()["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let comparison_path = directory.path().join("comparison.json");
    let mut comparison = OfficialComparison {
        schema_version: SCHEMA_VERSION,
        scenario,
        expected_versions: BTreeMap::from([("DEMO_ACT".into(), expected_version)]),
    };
    fs::write(&comparison_path, serde_json::to_vec(&comparison).unwrap()).unwrap();
    let matched = cli(&path, &["reconcile", comparison_path.to_str().unwrap()]);
    assert!(
        matched.status.success(),
        "{}",
        String::from_utf8_lossy(&matched.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&matched.stdout).unwrap()["matched_versions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    expected.tree = base.revision.tree.clone();
    expected.revision_id = "SYNTHETIC_MISMATCH".into();
    comparison.expected_versions.insert(
        "DEMO_ACT".into(),
        store.save_revision(&expected, chrono::Utc::now()).unwrap(),
    );
    fs::write(&comparison_path, serde_json::to_vec(&comparison).unwrap()).unwrap();
    let mismatched = cli(&path, &["reconcile", comparison_path.to_str().unwrap()]);
    assert!(!mismatched.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&mismatched.stdout).unwrap()["diagnostics"][0]["code"],
        "official_mismatch"
    );
}

#[test]
fn cli_server_reports_a_bound_listener_and_serves_real_requests() {
    use std::io::{BufRead, Read, Write};
    struct Server(std::process::Child);
    impl Drop for Server {
        fn drop(&mut self) {
            match self.0.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {}
                Err(error) => eprintln!("Cannot inspect fixture server: {error}"),
            }
            if let Err(error) = self.0.kill() {
                eprintln!("Cannot stop fixture server: {error}");
            }
            if let Err(error) = self.0.wait() {
                eprintln!("Cannot reap fixture server: {error}");
            }
        }
    }
    let (_directory, store) = imported();
    let child = Command::new(env!("CARGO_BIN_EXE_law-history"))
        .arg("--store")
        .arg(&store)
        .args(["serve", "--bind", "127.0.0.1:0"])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut server = Server(child);
    let mut reader = std::io::BufReader::new(server.0.stderr.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let address = line
        .trim()
        .strip_prefix("law-history listening on http://")
        .expect("Server must report its bound address");
    let deadline = std::time::Duration::from_secs(5);
    let mut stream =
        std::net::TcpStream::connect_timeout(&address.parse().unwrap(), deadline).unwrap();
    stream.set_read_timeout(Some(deadline)).unwrap();
    stream.set_write_timeout(Some(deadline)).unwrap();
    stream
        .write_all(b"GET /api/status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("stored_revisions"));
}

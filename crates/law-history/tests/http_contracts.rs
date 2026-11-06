use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::IntoResponse,
};
use law_history::{
    Store,
    amendment::{AmendmentStep, Scenario},
    api, ingest,
    model::*,
    service, xml,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tower::ServiceExt;

fn fixture() -> (tempfile::TempDir, Store, Router, Vec<StoredRevision>) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    ingest::import_bundle(&store, include_bytes!("../../../fixtures/demo.json")).unwrap();
    let history = store.history("DEMO_ACT", None).unwrap();
    let router = api::router(
        api::ApiState {
            root: directory.path().into(),
            today: Some("2026-11-05".parse().unwrap()),
        },
        None,
    );
    (directory, store, router, history)
}

async fn decode(response: axum::response::Response) -> Value {
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    serde_json::from_slice(
        &to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn get(router: &Router, uri: &str) -> Value {
    let response = router
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    decode(response).await
}

async fn post(router: &Router, uri: &str, body: &Value) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::post(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn read_routes_preserve_law_versions_dates_queries_and_source_metadata() {
    let (_directory, store, router, history) = fixture();
    let today = "2026-11-05".parse().unwrap();
    for (suffix, query) in [
        (String::new(), Query::default()),
        (
            "?at=1969-12-31".into(),
            Query {
                at: Some("1969-12-31".parse().unwrap()),
                ..Default::default()
            },
        ),
        (
            "?known_at=2026-01-01T12:00:00Z".into(),
            Query {
                known_at: Some("2026-01-01T12:00:00Z".parse().unwrap()),
                ..Default::default()
            },
        ),
        (
            format!("?version={}", history[0].version),
            Query {
                version: Some(history[0].version.clone()),
                ..Default::default()
            },
        ),
    ] {
        let expected = service::select(&store, "DEMO_ACT", &query, today).unwrap();
        assert_eq!(
            get(&router, &format!("/api/laws/DEMO_ACT{suffix}")).await,
            json!(expected)
        );
        assert_eq!(
            get(&router, &format!("/api/laws/DEMO_ACT/refs{suffix}")).await,
            json!(service::refs(&store, "DEMO_ACT", &query, today).unwrap())
        );
    }
    for (suffix, known_at) in [
        ("", None),
        (
            "?known_at=2026-01-01T12:00:00Z",
            Some("2026-01-01T12:00:00Z".parse().unwrap()),
        ),
    ] {
        assert_eq!(
            get(&router, &format!("/api/laws/DEMO_ACT/log{suffix}")).await,
            json!(store.history("DEMO_ACT", known_at).unwrap())
        );
        assert_eq!(
            get(&router, &format!("/api/laws/DEMO_ACT/parliament{suffix}")).await,
            json!(service::parliament(&store, "DEMO_ACT", known_at).unwrap())
        );
    }
    for q in ["", "fixture", "missing-title"] {
        assert_eq!(
            get(
                &router,
                &format!("/api/search?q={q}&at=1969-12-31&known_at=2026-01-01T12:00:00Z")
            )
            .await,
            json!(
                service::search(
                    &store,
                    q,
                    &Query {
                        at: Some("1969-12-31".parse().unwrap()),
                        known_at: Some("2026-01-01T12:00:00Z".parse().unwrap()),
                        version: None
                    },
                    today
                )
                .unwrap()
            )
        );
    }
    let from = &history[0].version;
    let to = &history[1].version;
    assert_eq!(
        get(&router, &format!("/api/diff?from={from}&to={to}")).await,
        json!(service::diff(&store, from, to).unwrap())
    );
    let query = Query {
        at: Some("1970-04-01".parse().unwrap()),
        ..Default::default()
    };
    assert_eq!(
        get(
            &router,
            &format!("/api/impact?from={from}&to={to}&at=1970-04-01")
        )
        .await,
        json!(service::impact(&store, from, to, &query, today).unwrap())
    );
    let path = xml::index(&history[1].revision.tree)
        .into_iter()
        .find(|(_, node)| node.tag == "Sentence")
        .unwrap()
        .0;
    let query = Query {
        version: Some(to.clone()),
        ..Default::default()
    };
    assert_eq!(
        get(
            &router,
            &format!("/api/laws/DEMO_ACT/blame?path={path}&version={to}")
        )
        .await,
        json!(service::blame(&store, "DEMO_ACT", &path, &query, today).unwrap())
    );
    assert_eq!(
        get(&router, "/api/status").await,
        json!(service::status(&store, None).unwrap())
    );
    let document = get(&router, "/api/openapi.json").await;
    assert!(document["paths"]["/api/laws/{law_id}"]["get"].is_object());
    assert!(document["components"]["schemas"]["Scenario"].is_object());
}

#[tokio::test]
async fn simulation_parse_and_explicit_scenario_persistence_round_trip() {
    let (_directory, store, router, history) = fixture();
    let scenario = Scenario {
        schema_version: SCHEMA_VERSION,
        name: "x".repeat(8192),
        bases: BTreeMap::from([("DEMO_ACT".into(), history[0].version.clone())]),
        at: "1970-04-01".parse().unwrap(),
        known_at: Some("2026-11-05T00:00:00Z".parse().unwrap()),
        reference_context: None,
        steps: vec![AmendmentStep {
            id: "no-edit".into(),
            effective: LegalDate::known("1970-04-01".parse().unwrap(), "fixture"),
            after: Vec::new(),
            operations: Vec::new(),
            adjustments: Vec::new(),
        }],
    };
    let response = post(&router, "/api/simulate", &json!(scenario)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let result = decode(response).await;
    assert_eq!(result, json!(service::simulate(&store, &scenario).unwrap()));
    let id = result["scenario_id"].as_str().unwrap();
    assert!(
        store
            .latest_fact::<Scenario>(&format!("scenario:{id}"), None)
            .unwrap()
            .is_none()
    );
    let response = post(&router, "/api/scenarios", &json!(scenario)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let saved_response = decode(response).await;
    let saved_id = saved_response["id"].as_str().unwrap();
    let saved: Scenario =
        serde_json::from_value(get(&router, &format!("/api/scenarios/{saved_id}")).await).unwrap();
    assert_eq!(
        saved_id,
        law_history::store::sha256(&serde_json::to_vec(&saved).unwrap())
    );
    assert_eq!(saved.bases, scenario.bases);
    assert_eq!(saved.known_at, scenario.known_at);
    assert!(saved.reference_context.is_some());
    assert_eq!(
        store
            .latest_fact::<Scenario>(&format!("scenario:{saved_id}"), None)
            .unwrap()
            .unwrap()
            .name,
        scenario.name
    );
    let input = json!({"law_id":"DEMO_ACT","base_version":history[0].version,"text":"第一条中「書面」を「電磁的記録」に改める。","source_url":"https://example.invalid/amendment"});
    let response = post(&router, "/api/amendments/parse", &input).await;
    assert_eq!(response.status(), StatusCode::OK);
    let parsed = decode(response).await;
    assert_eq!(parsed["diagnostics"], json!([]));
    assert_eq!(parsed["operations"].as_array().unwrap().len(), 1);
    assert_eq!(
        parsed["operations"][0]["evidence"]["url"],
        input["source_url"]
    );
    let digest = parsed["operations"][0]["evidence"]["raw_sha256"]
        .as_str()
        .unwrap();
    assert_eq!(
        store.source(digest).unwrap(),
        input["text"].as_str().unwrap().as_bytes()
    );
    let mut wrong = input;
    wrong["law_id"] = json!("DIFFERENT");
    let response = post(&router, "/api/amendments/parse", &wrong).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        decode(response).await["error"]
            .as_str()
            .unwrap()
            .contains("different law")
    );
}

#[tokio::test]
async fn malformed_missing_oversized_and_private_storage_errors_remain_distinct() {
    let (_directory, _store, router, _history) = fixture();
    for (uri, expected) in [
        ("/api/laws/MISSING", StatusCode::NOT_FOUND),
        ("/api/scenarios/missing", StatusCode::NOT_FOUND),
        ("/api/laws/DEMO_ACT?at=1900-02-29", StatusCode::BAD_REQUEST),
        ("/api/diff?from=invalid&to=invalid", StatusCode::BAD_REQUEST),
        ("/unknown", StatusCode::NOT_FOUND),
    ] {
        let response = router
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{uri}");
        assert!(decode(response).await["error"].is_string());
    }
    let response = post(
        &router,
        "/api/simulate",
        &json!({"padding":"x".repeat(2_097_153)}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(decode(response).await["error"].is_string());
    let cases = [
        (
            HistoryError::Invalid("invalid input".into()),
            StatusCode::BAD_REQUEST,
            "invalid input",
        ),
        (
            HistoryError::NotFound("missing record".into()),
            StatusCode::NOT_FOUND,
            "missing record",
        ),
        (
            HistoryError::Io(std::io::Error::other("private filesystem detail")),
            StatusCode::INTERNAL_SERVER_ERROR,
            "Storage operation failed",
        ),
        (
            HistoryError::Storage(rusqlite::Error::InvalidQuery),
            StatusCode::INTERNAL_SERVER_ERROR,
            "Storage operation failed",
        ),
        (
            HistoryError::Json(serde_json::from_str::<Value>("{").unwrap_err()),
            StatusCode::BAD_REQUEST,
            "EOF",
        ),
    ];
    for (error, expected_status, expected_text) in cases {
        let response = api::ApiError::from(error).into_response();
        assert_eq!(response.status(), expected_status);
        let body = decode(response).await;
        assert!(
            body["error"].as_str().unwrap().contains(expected_text),
            "{body}"
        );
        assert_eq!(body["diagnostics"], json!([]));
    }
    let diagnostics = vec![Diagnostic::new(
        DiagnosticCode::AmbiguousOrder,
        "Order needs evidence",
        "fixture",
        "/steps",
    )];
    let response =
        api::ApiError::from(HistoryError::Ambiguous(diagnostics.clone())).into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(decode(response).await["diagnostics"], json!(diagnostics));
    let response = router
        .clone()
        .oneshot(
            Request::get("/api/scenarios/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        decode(response).await,
        json!({"error":HistoryError::NotFound("Scenario not found".into()).to_string(),"diagnostics":[]})
    );
    for length in [16_384, 100_000] {
        let field = "x".repeat(length);
        let response = post(&router, "/api/simulate", &json!({field.clone():0})).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = decode(response).await;
        let error = body["error"].as_str().unwrap();
        if length < 65_536 {
            assert!(error.contains(&field));
        } else {
            assert!(error.contains("response limit"));
        }
    }
}

#[tokio::test]
async fn native_server_accepts_http_requests_and_static_routes_preserve_assets() {
    let (directory, _store, _router, _) = fixture();
    let web = directory.path().join("web");
    std::fs::create_dir(&web).unwrap();
    std::fs::write(web.join("index.html"), "<main>Static fixture</main>").unwrap();
    let router = api::router(
        api::ApiState {
            root: directory.path().into(),
            today: None,
        },
        Some(web),
    );
    let response = router
        .clone()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        &to_bytes(response.into_body(), 1024).await.unwrap()[..],
        b"<main>Static fixture</main>"
    );
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(api::serve(listener, router));
    let response = tokio::task::spawn_blocking(move || {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap()
            .get(format!("http://{address}/api/status"))
            .send()
            .unwrap()
            .json::<Value>()
            .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(response["laws"].as_array().unwrap().len(), 1);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

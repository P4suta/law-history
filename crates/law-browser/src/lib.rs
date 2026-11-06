use chrono::{DateTime, Utc};
use law_history::{
    amendment::Scenario, browser_ingest, ingest, memory_store::Store, model::*, service,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
const STATUS_OK: u16 = 200;
const STATUS_BAD_REQUEST: u16 = 400;
const STATUS_NOT_FOUND: u16 = 404;
const STATUS_CONFLICT: u16 = 409;

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Catalog {
        bytes: Vec<u8>,
    },
    Search {
        q: String,
        query: Query,
    },
    Show {
        law: String,
        query: Query,
    },
    History {
        law: String,
        query: Query,
    },
    Refs {
        law: String,
        query: Query,
    },
    Diff {
        from: String,
        to: String,
    },
    Impact {
        from: String,
        to: String,
        query: Query,
    },
    Parliament {
        law: String,
        query: Query,
    },
    Status {},
    Simulate {
        scenario: Scenario,
    },
    SaveScenario {
        scenario: Scenario,
    },
    LoadScenario {
        id: String,
    },
    SharedScenario {
        id: String,
        scenario: Scenario,
    },
    Import {
        bytes: Vec<u8>,
    },
    CollectHistory {
        law: String,
        bytes: Vec<u8>,
    },
    CollectRevision {
        law: String,
        revision: String,
        history: String,
        bytes: Vec<u8>,
    },
}

#[derive(Serialize, Deserialize)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
}

#[derive(Default)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Engine {
    store: Store,
}

fn value<T: Serialize>(result: Result<T>) -> Result<Value> {
    Ok(serde_json::to_value(result?)?)
}

fn checked_scenario(id: &str, scenario: Scenario) -> Result<Value> {
    if scenario.schema_version != SCHEMA_VERSION
        || scenario.known_at.is_none()
        || scenario.reference_context.is_none()
    {
        return Err(HistoryError::Invalid(
            "A shared scenario requires fixed observation and reference conditions".into(),
        ));
    }
    if law_history::store_types::sha256(&serde_json::to_vec(&scenario)?) != id {
        return Err(HistoryError::Invalid(
            "Scenario identity differs from its pinned recipe".into(),
        ));
    }
    Ok(serde_json::to_value(scenario)?)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Engine {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_effect(request: &str) -> String {
        if !law_domain::input_fits(request.len(), MAX_REQUEST_BYTES) {
            return "invalid".into();
        }
        match serde_json::from_str::<Request>(request) {
            Ok(Request::Import { .. }) => "import",
            Ok(
                Request::CollectHistory { .. }
                | Request::CollectRevision { .. }
                | Request::SaveScenario { .. },
            ) => "write",
            Ok(
                Request::Catalog { .. }
                | Request::Search { .. }
                | Request::Show { .. }
                | Request::History { .. }
                | Request::Refs { .. }
                | Request::Diff { .. }
                | Request::Impact { .. }
                | Request::Parliament { .. }
                | Request::Status {}
                | Request::Simulate { .. }
                | Request::LoadScenario { .. }
                | Request::SharedScenario { .. },
            ) => "read",
            Err(_) => "invalid",
        }
        .into()
    }

    pub fn execute(&self, request: &str, now: &str) -> String {
        let reply = match self.run(request, now) {
            Ok(body) => Reply {
                status: STATUS_OK,
                body,
            },
            Err(error) => {
                let status = match &error {
                    HistoryError::NotFound(_) => STATUS_NOT_FOUND,
                    HistoryError::Ambiguous(_) => STATUS_CONFLICT,
                    _ => STATUS_BAD_REQUEST,
                };
                let diagnostics = if let HistoryError::Ambiguous(items) = &error {
                    items.clone()
                } else {
                    Vec::new()
                };
                Reply {
                    status,
                    body: json!({"error":error.to_string(),"diagnostics":diagnostics}),
                }
            }
        };
        serde_json::to_string(&reply).expect("A JSON reply contains only JSON values")
    }

    pub fn snapshot(&self) -> std::result::Result<Vec<u8>, String> {
        self.store.export().map_err(|error| error.to_string())
    }

    pub fn restore(&self, bytes: &[u8]) -> std::result::Result<(), String> {
        self.store.restore(bytes).map_err(|error| error.to_string())
    }
}

impl Engine {
    fn run(&self, request: &str, now: &str) -> Result<Value> {
        if !law_domain::input_fits(request.len(), MAX_REQUEST_BYTES) {
            return Err(HistoryError::Invalid(
                "Request exceeds the 64 MiB input limit".into(),
            ));
        }
        let now: DateTime<Utc> = now
            .parse()
            .map_err(|_| HistoryError::Invalid("Invalid collection timestamp".into()))?;
        let today = japan_day(now)?;
        let request: Request = serde_json::from_str(request)?;
        match request {
            Request::Catalog { bytes } => Ok(serde_json::to_value(
                browser_ingest::catalog(&bytes)
                    .map_err(|error| HistoryError::Invalid(error.to_string()))?,
            )?),
            Request::Search { q, query } => value(service::search(&self.store, &q, &query, today)),
            Request::Show { law, query } => {
                value(service::select(&self.store, &law, &query, today))
            }
            Request::History { law, query } => value(self.store.history(&law, query.known_at)),
            Request::Refs { law, query } => value(service::refs(&self.store, &law, &query, today)),
            Request::Diff { from, to } => value(service::diff(&self.store, &from, &to)),
            Request::Impact { from, to, query } => {
                value(service::impact(&self.store, &from, &to, &query, today))
            }
            Request::Parliament { law, query } => {
                value(service::parliament(&self.store, &law, query.known_at))
            }
            Request::Status {} => value(service::status(&self.store, None)),
            Request::Simulate { scenario } => value(service::simulate(&self.store, &scenario)),
            Request::SaveScenario { scenario } => self.transaction(|| {
                Ok(json!({"id":service::save_scenario(&self.store,&scenario,now)?}))
            }),
            Request::LoadScenario { id } => {
                let scenario = self
                    .store
                    .latest_fact::<Scenario>(&format!("scenario:{id}"), None)?
                    .ok_or_else(|| {
                        HistoryError::NotFound("Scenario is absent from the browser corpus".into())
                    })?;
                checked_scenario(&id, scenario)
            }
            Request::SharedScenario { id, scenario } => checked_scenario(&id, scenario),
            Request::Import { bytes } => self.transaction(|| {
                if serde_json::from_slice::<Value>(&bytes)?.is_array() {
                    ingest::import_bundle(&self.store, &bytes)
                        .map_err(|error| HistoryError::Invalid(format!("{error:#}")))?;
                } else {
                    self.store.restore(&bytes)?;
                }
                value(service::status(&self.store, None))
            }),
            Request::CollectHistory { law, bytes } => self.transaction(|| {
                let inventory = browser_ingest::history(&self.store, &law, &bytes, now)
                    .map_err(|error| HistoryError::Invalid(format!("{error:#}")))?;
                Ok(serde_json::to_value(inventory)?)
            }),
            Request::CollectRevision {
                law,
                revision,
                history,
                bytes,
            } => self.transaction(|| {
                let version =
                    browser_ingest::revision(&self.store, &law, &revision, &history, &bytes, now)
                        .map_err(|error| HistoryError::Invalid(format!("{error:#}")))?;
                Ok(json!({"version":version}))
            }),
        }
    }

    fn transaction(&self, operation: impl FnOnce() -> Result<Value>) -> Result<Value> {
        let before = self.store.export()?;
        match operation().and_then(|result| {
            self.store.export()?;
            Ok(result)
        }) {
            Ok(result) => Ok(result),
            Err(error) => {
                self.store.restore(&before)?;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(engine: &Engine, request: Value) -> Reply {
        serde_json::from_str(&engine.execute(&request.to_string(), "2026-11-06T00:00:00Z")).unwrap()
    }

    #[test]
    fn browser_engine_runs_shared_history_rules_and_restores_exact_versions() {
        let engine = Engine::new();
        assert_eq!(call(&engine,json!({"operation":"import","bytes":include_bytes!("../../../fixtures/demo.json").to_vec()})).status,200);
        let history = call(
            &engine,
            json!({"operation":"history","law":"DEMO_ACT","query":{}}),
        );
        assert_eq!(history.body.as_array().unwrap().len(), 2);
        let selected = call(
            &engine,
            json!({"operation":"show","law":"DEMO_ACT","query":{"at":"1969-12-31"}}),
        );
        assert_eq!(selected.status, 200);
        assert!(
            selected.body["selected"]["revision"]["tree"]
                .to_string()
                .contains("書面")
        );
        let diff = call(
            &engine,
            json!({"operation":"diff","from":history.body[0]["version"],"to":history.body[1]["version"]}),
        );
        assert_eq!(diff.status, 200);
        assert!(!diff.body["changes"].as_array().unwrap().is_empty());
        let restored = Engine::new();
        restored.restore(&engine.snapshot().unwrap()).unwrap();
        assert_eq!(
            call(
                &restored,
                json!({"operation":"history","law":"DEMO_ACT","query":{}})
            )
            .body,
            history.body
        );
        assert_eq!(
            call(
                &restored,
                json!({"operation":"search","q":"DEMO","query":{}})
            )
            .body["hits"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            call(
                &restored,
                json!({"operation":"refs","law":"DEMO_ACT","query":{}})
            )
            .status,
            200
        );
    }

    #[test]
    fn browser_engine_rejects_invalid_input_without_losing_its_corpus() {
        let engine = Engine::new();
        let before = engine.snapshot().unwrap();
        for request in [
            json!({"operation":"import","bytes":[91,123]}),
            json!({"operation":"collect_history","law":"OTHER","bytes":b"{}".to_vec()}),
            json!({"operation":"show","law":"NONE","query":{}}),
            json!({"operation":"load_scenario","id":"absent"}),
            json!({"operation":"unknown"}),
        ] {
            assert!(call(&engine, request).status >= 400);
            assert_eq!(engine.snapshot().unwrap(), before);
        }
        assert_eq!(
            serde_json::from_str::<Reply>(&engine.execute("{}", "invalid"))
                .unwrap()
                .status,
            400
        );
        assert!(engine.restore(b"{}").is_err());
        assert_eq!(engine.snapshot().unwrap(), before);
    }

    #[test]
    fn request_effects_classify_every_operation_and_refuse_invalid_envelopes() {
        let scenario = json!({"schema_version":1,"name":"Scope fixture","bases":{},"at":"1970-01-01","steps":[]});
        let writes = [
            json!({"operation":"collect_history","law":"LAW","bytes":[]}),
            json!({"operation":"collect_revision","law":"LAW","revision":"REVISION","history":"digest","bytes":[]}),
            json!({"operation":"save_scenario","scenario":scenario}),
        ];
        for command in writes {
            assert_eq!(Engine::request_effect(&command.to_string()), "write");
        }
        assert_eq!(
            Engine::request_effect(&json!({"operation":"import","bytes":[]}).to_string()),
            "import"
        );
        for command in [
            json!({"operation":"catalog","bytes":[]}),
            json!({"operation":"search","q":"","query":{}}),
            json!({"operation":"show","law":"LAW","query":{}}),
            json!({"operation":"history","law":"LAW","query":{}}),
            json!({"operation":"refs","law":"LAW","query":{}}),
            json!({"operation":"diff","from":"before","to":"after"}),
            json!({"operation":"impact","from":"before","to":"after","query":{}}),
            json!({"operation":"parliament","law":"LAW","query":{}}),
            json!({"operation":"status"}),
            json!({"operation":"simulate","scenario":scenario}),
            json!({"operation":"load_scenario","id":"id"}),
            json!({"operation":"shared_scenario","id":"id","scenario":scenario}),
        ] {
            assert_eq!(
                Engine::request_effect(&command.to_string()),
                "read",
                "{command}"
            );
        }
        for command in [
            "{}",
            "{",
            "null",
            "{\"operation\":\"unknown\"}",
            "{\"operation\":\"status\",\"ignore_conflicts\":true}",
        ] {
            assert_eq!(Engine::request_effect(command), "invalid");
        }
        let catalog = json!({"total_count":1,"laws":[{"law_info":{"law_id":"LAW","law_num":"Fixture"},"revision_info":{"law_title":"<Ruby>法<Rt>ほう</Rt></Ruby>律"}}]});
        let result = call(
            &Engine::new(),
            json!({"operation":"catalog","bytes":serde_json::to_vec(&catalog).unwrap()}),
        );
        assert_eq!(result.status, 200);
        assert_eq!(result.body["laws"][0]["revision_info"]["law_title"], "法律");
    }

    #[test]
    fn engine_dispatch_preserves_dates_filters_scenarios_and_impact() {
        let engine = Engine::new();
        assert_eq!(call(&engine,json!({"operation":"import","bytes":include_bytes!("../../../fixtures/demo.json").to_vec()})).status,200);
        let history = call(
            &engine,
            json!({"operation":"history","law":"DEMO_ACT","query":{}}),
        )
        .body;
        let before = history[0]["version"].clone();
        let after = history[1]["version"].clone();
        for (now, expected) in [
            ("1970-03-31T14:59:59Z", &before),
            ("1970-03-31T15:00:00Z", &after),
        ] {
            let selected: Reply = serde_json::from_str(&engine.execute(
                &json!({"operation":"show","law":"DEMO_ACT","query":{}}).to_string(),
                now,
            ))
            .unwrap();
            assert_eq!(&selected.body["selected"]["version"], expected);
        }
        assert_eq!(
            call(
                &engine,
                json!({"operation":"history","law":"OTHER","query":{}})
            )
            .body,
            json!([])
        );
        assert_eq!(call(&engine,json!({"operation":"history","law":"DEMO_ACT","query":{"known_at":"1969-12-31T00:00:00Z"}})).body,json!([]));
        assert_eq!(
            call(
                &engine,
                json!({"operation":"search","q":"missing","query":{}})
            )
            .body["hits"],
            json!([])
        );
        assert_eq!(
            call(&engine, json!({"operation":"status"})).body["laws"][0]["law_id"],
            "DEMO_ACT"
        );
        let impact = call(
            &engine,
            json!({"operation":"impact","from":before,"to":after,"query":{}}),
        );
        assert_eq!(impact.status, 200);
        assert!(!impact.body["direct"].as_array().unwrap().is_empty());
        let scenario = json!({"schema_version":1,"name":"Pinned browser scenario","bases":{"DEMO_ACT":before},"at":"1970-04-01","known_at":"2026-11-06T00:00:00Z","reference_context":null,"steps":[]});
        let simulation = call(&engine, json!({"operation":"simulate","scenario":scenario}));
        assert_eq!(simulation.status, 200);
        assert_eq!(
            simulation.body["results"]["DEMO_ACT"]["revision_id"],
            history[0]["revision"]["revision_id"]
        );
        assert_eq!(
            call(&engine, json!({"operation":"load_scenario","id":"absent"})).status,
            404
        );
        let saved = call(
            &engine,
            json!({"operation":"save_scenario","scenario":scenario}),
        );
        assert_eq!(saved.status, 200);
        let id = saved.body["id"].as_str().unwrap();
        assert_eq!(id.len(), 64);
        let loaded = call(&engine, json!({"operation":"load_scenario","id":id}));
        assert_eq!(loaded.status, 200);
        assert_eq!(loaded.body["name"], "Pinned browser scenario");
        assert_eq!(loaded.body["bases"]["DEMO_ACT"], before);
        assert!(loaded.body["reference_context"]["versions"].is_object());
        assert_eq!(
            call(
                &engine,
                json!({"operation":"shared_scenario","id":id,"scenario":loaded.body})
            )
            .body,
            loaded.body
        );
        let mut altered = loaded.body.clone();
        altered["name"] = json!("Different recipe");
        assert_eq!(
            call(
                &engine,
                json!({"operation":"shared_scenario","id":id,"scenario":altered})
            )
            .status,
            400
        );
        for field in ["known_at", "reference_context"] {
            let mut unpinned = loaded.body.clone();
            unpinned[field] = Value::Null;
            assert_eq!(
                call(
                    &engine,
                    json!({"operation":"shared_scenario","id":id,"scenario":unpinned})
                )
                .status,
                400
            );
        }
        let wrong = "0".repeat(64);
        engine
            .store
            .fact(
                &format!("scenario:{wrong}"),
                &loaded.body,
                "2026-11-06T00:00:00Z".parse().unwrap(),
            )
            .unwrap();
        assert_eq!(
            call(&engine, json!({"operation":"load_scenario","id":wrong})).status,
            400
        );
        let original = engine.snapshot().unwrap();
        let mut partial: Value =
            serde_json::from_slice(include_bytes!("../../../fixtures/demo.json")).unwrap();
        partial[1]["law_id"] = json!("../invalid");
        assert_eq!(
            call(
                &engine,
                json!({"operation":"import","bytes":serde_json::to_vec(&partial).unwrap()})
            )
            .status,
            400
        );
        assert_eq!(engine.snapshot().unwrap(), original);
        assert_eq!(
            call(
                &engine,
                json!({"operation":"diff","from":"missing","to":after})
            )
            .status,
            404
        );
    }

    #[test]
    fn browser_requests_include_their_exact_encoded_size_boundary() {
        let engine = Engine::new();
        let mut request = String::from("{\"operation\":\"status\"}");
        request.extend(std::iter::repeat_n(' ', MAX_REQUEST_BYTES - request.len()));
        assert_eq!(Engine::request_effect(&request), "read");
        let reply: Reply =
            serde_json::from_str(&engine.execute(&request, "2026-11-06T00:00:00Z")).unwrap();
        assert_eq!(reply.status, 200);
        request.push(' ');
        assert_eq!(Engine::request_effect(&request), "invalid");
        let reply: Reply =
            serde_json::from_str(&engine.execute(&request, "2026-11-06T00:00:00Z")).unwrap();
        assert_eq!(reply.status, 400);
        assert!(reply.body["error"].as_str().unwrap().contains("64 MiB"));
    }

    #[test]
    fn ambiguous_history_uses_conflict_status_and_preserves_diagnostics() {
        let engine = Engine::new();
        let mut bundle: Value =
            serde_json::from_slice(include_bytes!("../../../fixtures/demo.json")).unwrap();
        assert_eq!(
            call(
                &engine,
                json!({"operation":"import","bytes":serde_json::to_vec(&bundle).unwrap()})
            )
            .status,
            200
        );
        bundle[0]["xml"] = json!(
            bundle[0]["xml"]
                .as_str()
                .unwrap()
                .replace("書面", "異なる原文")
        );
        assert_eq!(
            call(
                &engine,
                json!({"operation":"import","bytes":serde_json::to_vec(&bundle).unwrap()})
            )
            .status,
            200
        );
        let selected = call(
            &engine,
            json!({"operation":"show","law":"DEMO_ACT","query":{"at":"1969-12-31"}}),
        );
        assert_eq!(selected.status, 409);
        assert!(!selected.body["diagnostics"].as_array().unwrap().is_empty());
        assert!(
            selected.body["error"]
                .as_str()
                .unwrap()
                .contains("ambiguous")
        );
    }

    #[test]
    fn collection_cannot_commit_a_corpus_that_cannot_be_restored() {
        let engine = Engine::new();
        {
            let store = Store::default();
            let mut bytes = vec![0; 56 * 1024 * 1024];
            for index in 0..2 {
                bytes[0] = index;
                store.preserve(&bytes).unwrap();
            }
            engine.restore(&store.export().unwrap()).unwrap();
        }
        let before = engine.snapshot().unwrap();
        let history = json!({"law_info":{"law_id":"LIMIT_ACT","law_num":"Synthetic limit fixture","promulgation_date":"1969-01-01"},"revisions":[{"law_revision_id":"LIMIT_REVISION","law_title":"Limit fixture","updated":"2026-11-06T00:00:00Z","amendment_type":"1","mission":"New","repeal_status":"None","current_revision_status":"CurrentEnforced"}]}).to_string();
        let mut bytes = history.into_bytes();
        bytes.resize(20 * 1024 * 1024, b' ');
        #[derive(Serialize)]
        struct CollectHistory<'a> {
            operation: &'static str,
            law: &'static str,
            bytes: &'a [u8],
        }
        let request = serde_json::to_string(&CollectHistory {
            operation: "collect_history",
            law: "LIMIT_ACT",
            bytes: &bytes,
        })
        .unwrap();
        drop(bytes);
        let reply: Reply =
            serde_json::from_str(&engine.execute(&request, "2026-11-06T00:00:00Z")).unwrap();
        assert_eq!(reply.status, 400);
        assert!(
            reply.body["error"]
                .as_str()
                .unwrap()
                .contains("Snapshot exceeds")
        );
        assert!(
            engine.snapshot().unwrap() == before,
            "Failed collection changed the corpus"
        );
        let recovered = Engine::new();
        recovered.restore(&before).unwrap();
        assert!(
            recovered.snapshot().unwrap() == before,
            "The retained corpus is not restorable"
        );
    }
}

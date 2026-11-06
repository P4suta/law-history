use law_browser::{Engine, Reply};
use law_history::{
    amendment::{PinnedReferenceContext, Scenario},
    memory_store::Store,
    store_types::sha256,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn call(engine: &Engine, request: Value) -> Reply {
    serde_json::from_str(&engine.execute(&request.to_string(), "2026-11-06T00:00:00Z")).unwrap()
}

fn corpus() -> (Engine, String) {
    let engine = Engine::new();
    assert_eq!(
        call(
            &engine,
            json!({"operation":"import","bytes":include_bytes!("../../../fixtures/demo.json").to_vec()}),
        )
        .status,
        200
    );
    let history = call(
        &engine,
        json!({"operation":"history","law":"DEMO_ACT","query":{}}),
    );
    assert_eq!(history.status, 200);
    let base = history.body[0]["version"].as_str().unwrap().to_owned();
    (engine, base)
}

fn scenario(base: &str, schema_version: u32, known: bool, references: bool) -> Scenario {
    let bases = BTreeMap::from([("DEMO_ACT".into(), base.into())]);
    Scenario {
        schema_version,
        name: "Scenario conditions".into(),
        bases: bases.clone(),
        at: "1969-04-01".parse().unwrap(),
        known_at: known.then(|| "2026-11-06T00:00:00Z".parse().unwrap()),
        reference_context: references.then_some(PinnedReferenceContext {
            versions: bases,
            diagnostics: Vec::new(),
        }),
        steps: Vec::new(),
    }
}

fn imported_scenario(id: &str, scenario: &Scenario) -> Engine {
    let store = Store::default();
    store
        .fact(
            &format!("scenario:{id}"),
            scenario,
            "2026-11-06T00:00:00Z".parse().unwrap(),
        )
        .unwrap();
    let engine = Engine::new();
    engine.restore(&store.export().unwrap()).unwrap();
    engine
}

#[test]
fn valid_content_addresses_keep_every_scenario_condition_independent() {
    let (source, base) = corpus();
    let original = source.snapshot().unwrap();
    let mut executed = 0;
    for version in [0, 1, 2, u32::MAX] {
        for known in [false, true] {
            for references in [false, true] {
                let scenario = scenario(&base, version, known, references);
                let id = sha256(&serde_json::to_vec(&scenario).unwrap());
                let receiver = imported_scenario(&id, &scenario);
                let imported = receiver.snapshot().unwrap();
                let expected = match (version, known, references) {
                    (1, true, true) => 200,
                    _ => 400,
                };
                for reply in [
                    call(
                        &source,
                        json!({"operation":"shared_scenario","id":id,"scenario":scenario}),
                    ),
                    call(&receiver, json!({"operation":"load_scenario","id":id})),
                ] {
                    assert_eq!(
                        reply.status, expected,
                        "schema={version}, known={known}, references={references}"
                    );
                    if expected == 200 {
                        assert_eq!(reply.body, serde_json::to_value(&scenario).unwrap());
                    } else {
                        assert_eq!(
                            reply.body["error"],
                            "A shared scenario requires fixed observation and reference conditions"
                        );
                    }
                    executed += 1;
                }
                assert_eq!(receiver.snapshot().unwrap(), imported);
            }
        }
    }
    assert_eq!(executed, 32);
    assert_eq!(source.snapshot().unwrap(), original);
}

#[test]
fn scenario_identity_is_checked_when_all_required_conditions_hold() {
    let (source, base) = corpus();
    let scenario = scenario(&base, 1, true, true);
    let wrong = "0".repeat(64);
    assert_ne!(sha256(&serde_json::to_vec(&scenario).unwrap()), wrong);
    let receiver = imported_scenario(&wrong, &scenario);
    for reply in [
        call(
            &source,
            json!({"operation":"shared_scenario","id":wrong,"scenario":scenario}),
        ),
        call(&receiver, json!({"operation":"load_scenario","id":wrong})),
    ] {
        assert_eq!(reply.status, 400);
        assert_eq!(
            reply.body["error"],
            "Scenario identity differs from its pinned recipe"
        );
    }
}

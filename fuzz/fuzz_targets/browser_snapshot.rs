#![no_main]
use law_browser::Engine;
use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;

static POPULATED: OnceLock<Vec<u8>> = OnceLock::new();

fn populated() -> &'static [u8] {
    POPULATED.get_or_init(|| {
        let store = law_history::memory_store::Store::default();
        law_history::ingest::import_bundle(&store, include_bytes!("../../fixtures/demo.json"))
            .unwrap();
        let revisions: Vec<_> = store
            .history("DEMO_ACT", None)
            .unwrap()
            .into_iter()
            .map(|row| row.revision)
            .collect();
        let now = "2026-11-06T00:00:00Z".parse().unwrap();
        store.save_revisions(&revisions, now).unwrap();
        store
            .fact("fact:fuzz", &serde_json::json!({"fixture":true}), now)
            .unwrap();
        store.export().unwrap()
    })
}

fuzz_target!(|bytes: &[u8]| {
    let engine = Engine::new();
    engine.restore(populated()).unwrap();
    let before = engine.snapshot().unwrap();
    if engine.restore(bytes).is_err() {
        assert_eq!(engine.snapshot().unwrap(), before);
    } else {
        let recovered = Engine::new();
        recovered.restore(&engine.snapshot().unwrap()).unwrap();
        assert_eq!(recovered.snapshot().unwrap(), engine.snapshot().unwrap());
        let reply = engine.execute("{\"operation\":\"status\"}", "2026-11-06T00:00:00Z");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&reply).unwrap()["status"],
            200
        );
    }
    if let Some(schema) = bytes.first() {
        let mut snapshot: serde_json::Value = serde_json::from_slice(populated()).unwrap();
        let records = snapshot["records"].as_object_mut().unwrap();
        let key = records
            .keys()
            .nth(usize::from(*schema) % records.len())
            .unwrap()
            .clone();
        let old = records.remove(&key).unwrap();
        let raw: Vec<u8> = serde_json::from_value(old).unwrap();
        let mut record: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        record["schema_version"] = serde_json::json!(schema);
        let raw = serde_json::to_vec(&record).unwrap();
        records.insert(law_history::store::sha256(&raw), serde_json::json!(raw));
        let candidate = Engine::new();
        candidate.restore(populated()).unwrap();
        if candidate
            .restore(&serde_json::to_vec(&snapshot).unwrap())
            .is_err()
        {
            assert_eq!(candidate.snapshot().unwrap(), populated());
        }
    }
    if let Ok(request) = std::str::from_utf8(bytes) {
        let result = engine.execute(request, "2026-11-06T00:00:00Z");
        let reply: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(
            reply["status"]
                .as_u64()
                .is_some_and(|status| (200..500).contains(&status))
        );
    }
    let recovered = Engine::new();
    recovered.restore(&engine.snapshot().unwrap()).unwrap();
    assert_eq!(recovered.snapshot().unwrap(), engine.snapshot().unwrap());
});

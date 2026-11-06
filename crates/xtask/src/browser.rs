use anyhow::{Context, bail};
use law_browser::Engine;
use law_history::memory_store::Store;
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn build(root: &Path) -> anyhow::Result<()> {
    dependencies(root, std::env::var_os("LAW_HISTORY_NODE_MODULES"))?;
    let target = std::env::var_os("LAW_HISTORY_WASM_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("CARGO_TARGET_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| root.join("target"))
                .join("browser-wasm")
        });
    super::run(
        root,
        "cargo",
        &[
            "build",
            "--locked",
            "--release",
            "--package",
            "law-browser",
            "--target",
            "wasm32-unknown-unknown",
            "--target-dir",
            target.to_str().context("Non-UTF-8 target directory")?,
        ],
    )?;
    let wasm = target.join("wasm32-unknown-unknown/release/law_browser.wasm");
    super::run(
        root,
        "wasm-bindgen",
        &[
            wasm.to_str().context("Non-UTF-8 WebAssembly path")?,
            "--target",
            "web",
            "--out-dir",
            "web/src/wasm",
            "--out-name",
            "law_browser",
        ],
    )?;
    super::run(
        &root.join("web"),
        "npm",
        &[
            "run",
            "build",
            "--",
            "--mode",
            "pages",
            "--outDir",
            "dist-pages",
        ],
    )?;
    let output = root.join("web/dist-pages");
    fs::copy(root.join("web/openapi.json"), output.join("openapi.json"))?;
    fs::copy(root.join("fixtures/demo.json"), output.join("demo.json"))?;
    fs::write(output.join(".nojekyll"), b"")?;
    Ok(())
}

fn dependencies(root: &Path, cache: Option<std::ffi::OsString>) -> anyhow::Result<()> {
    let destination = root.join("web/node_modules");
    if !destination.exists() {
        let cache =
            cache.context("Install Web dependencies with npm ci --prefix web --ignore-scripts")?;
        let cache = fs::canonicalize(cache)?;
        if !cache.is_dir() {
            bail!("The Web dependency cache is not a directory");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&cache, &destination)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&cache, &destination)?;
    }
    Ok(())
}

pub fn serve(root: &Path, port: u16) -> anyhow::Result<()> {
    let router = axum::Router::new().nest_service(
        "/law-history",
        tower_http::services::ServeDir::new(root.join("web/dist-pages")),
    );
    tokio::runtime::Runtime::new()?.block_on(async move {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        axum::serve(listener, router).await.map_err(Into::into)
    })
}

fn model_cases() -> anyhow::Result<usize> {
    let times = [
        "1969-12-31T23:59:59Z",
        "1970-01-01T00:00:00Z",
        "2026-11-06T00:00:00Z",
    ];
    let mut executed = 0;
    let mut visited = std::collections::BTreeSet::new();
    for first in 0..times.len() {
        for second in 0..times.len() {
            for reverse in [false, true] {
                let store = Store::default();
                let mut expected = std::collections::BTreeMap::new();
                let mut records = vec![(times[first], 0), (times[second], 1)];
                if reverse {
                    records.reverse();
                }
                for (time, index) in &records {
                    let value = json!({"value":index});
                    store.fact("fact:model", &value, time.parse()?)?;
                }
                let recovered = Store::default();
                recovered.restore(&store.export()?)?;
                for cutoff in times {
                    let observed =
                        recovered.latest_fact::<Value>("fact:model", Some(cutoff.parse()?))?;
                    for (time, index) in &records {
                        if *time <= cutoff {
                            let record = format!(
                                "{{\"kind\":\"fact\",\"schema_version\":1,\"key\":\"fact:model\",\"value\":{{\"value\":{index}}},\"observed_at\":\"{time}\"}}"
                            );
                            let digest = law_history::store::sha256(record.as_bytes());
                            expected.insert((*time, digest), json!({"value":index}));
                        }
                    }
                    require_equal(
                        observed,
                        expected.last_key_value().map(|(_, value)| value.clone()),
                    )?;
                    visited.insert((records.clone(), cutoff));
                    executed += 1;
                }
                let engine = Engine::new();
                engine
                    .restore(&recovered.export()?)
                    .map_err(anyhow::Error::msg)?;
                require_bytes(
                    &engine.snapshot().map_err(anyhow::Error::msg)?,
                    &recovered.export()?,
                )?;
            }
        }
    }
    const EXPECTED_MODEL_CASES: usize = 54;
    require_equal(
        Some(json!(visited.len())),
        Some(json!(EXPECTED_MODEL_CASES)),
    )?;
    Ok(executed)
}

fn require_equal(observed: Option<Value>, expected: Option<Value>) -> anyhow::Result<()> {
    if observed != expected {
        bail!("The browser result differs from the independent event model");
    }
    Ok(())
}

fn require_bytes(observed: &[u8], expected: &[u8]) -> anyhow::Result<()> {
    if observed != expected {
        bail!("The browser engine changed canonical records on recovery");
    }
    Ok(())
}

pub fn model(root: &Path) -> anyhow::Result<()> {
    let sources = super::source_inventory(root, true, "crates/law-domain/src/lib.rs")?;
    let source = law_history::store::sha256(&serde_json::to_vec(&sources)?);
    let plan = law_history::store::sha256(b"browser-store-record-order-and-recovery");
    let executed = model_cases()?;
    if sources != super::source_inventory(root, true, "crates/law-domain/src/lib.rs")? {
        bail!("Source changed during model verification");
    }
    println!(
        "{}",
        serde_json::to_string(&json!({
            "version":1,
            "source":std::env::var("DOTFILES_VERIFICATION_SOURCE").unwrap_or(source),
            "plan":std::env::var("DOTFILES_VERIFICATION_PLAN").unwrap_or(plan),
            "method":"model","scope":"affected","tool_version":env!("CARGO_PKG_VERSION"),
            "target":"browser-store","bounds":"all 3×3 timestamp pairs, both record orders, and all 3 cutoffs",
            "results":[{"name":"browser-store-record-order-and-recovery","status":"passed","executed":executed,"failed":0,"skipped":0,"inconclusive":0}]
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn event_model_checks_both_execution_orders_and_every_cutoff() {
        assert_eq!(super::model_cases().unwrap(), 54);
    }

    #[test]
    fn model_verdicts_refuse_missing_different_and_changed_results() {
        use serde_json::json;
        assert!(super::require_equal(None, None).is_ok());
        assert!(super::require_equal(Some(json!(1)), Some(json!(1))).is_ok());
        assert!(super::require_equal(None, Some(json!(1))).is_err());
        assert!(super::require_equal(Some(json!(1)), None).is_err());
        assert!(super::require_equal(Some(json!(2)), Some(json!(1))).is_err());
        assert!(super::require_bytes(b"same", b"same").is_ok());
        assert!(super::require_bytes(b"other", b"same").is_err());
        assert!(super::require_bytes(b"", b"same").is_err());
    }

    #[test]
    fn dependency_setup_refuses_absent_missing_and_nondirectory_caches() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("web")).unwrap();
        assert!(super::dependencies(root.path(), None).is_err());
        assert!(
            super::dependencies(
                root.path(),
                Some(root.path().join("missing").into_os_string())
            )
            .is_err()
        );
        let file = root.path().join("file");
        std::fs::write(&file, "not a cache").unwrap();
        assert!(
            super::dependencies(root.path(), Some(file.into_os_string()))
                .unwrap_err()
                .to_string()
                .contains("not a directory")
        );
        let cache = root.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        super::dependencies(root.path(), Some(cache.clone().into_os_string())).unwrap();
        assert_eq!(
            std::fs::canonicalize(root.path().join("web/node_modules")).unwrap(),
            std::fs::canonicalize(cache).unwrap()
        );
        super::dependencies(root.path(), None).unwrap();
    }
}

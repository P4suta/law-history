use std::{fs, path::Path, process::Command};

#[test]
fn published_browser_bundle_runs_without_native_api() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = tempfile::tempdir().unwrap();
    let expected = law_browser::Engine::new();
    let request = serde_json::json!({"operation":"import","bytes":include_bytes!("../../../fixtures/demo.json").to_vec()});
    let result: law_browser::Reply =
        serde_json::from_str(&expected.execute(&request.to_string(), "2026-11-06T00:00:00Z"))
            .unwrap();
    assert_eq!(result.status, 200);
    let expected_file = fixture.path().join("expected-corpus.json");
    fs::write(&expected_file, expected.snapshot().unwrap()).unwrap();
    let output = root.join("web/dist-pages");
    if output.exists() {
        fs::remove_dir_all(&output).expect("Remove the previous test artifact");
    }
    let build = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("build-browser")
        .current_dir(&root)
        .output()
        .expect("Run the browser build command");
    assert!(
        build.status.success(),
        "The static browser bundle must build:\n{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let assets = root.join("web/dist-pages/assets");
    let wasm_files: Vec<_> = fs::read_dir(&assets)
        .expect("Read the published browser assets")
        .map(|entry| entry.expect("Read a browser asset").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "wasm")
        })
        .collect();
    assert_eq!(wasm_files.len(), 1, "Publish the actual Rust engine");
    let bytes = fs::read(&wasm_files[0]).expect("Read the WebAssembly module");
    assert!(bytes.starts_with(b"\0asm\x01\0\0\0"));
    let browser = Command::new("npm")
        .args([
            "exec",
            "--",
            "playwright",
            "test",
            "--config",
            "playwright.pages.config.ts",
        ])
        .current_dir(root.join("web"))
        .env("LAW_HISTORY_XTASK", env!("CARGO_BIN_EXE_xtask"))
        .env("LAW_HISTORY_EXPECTED_CORPUS", expected_file)
        .status()
        .expect("Run the browser-only behavior specifications");
    assert!(
        browser.success(),
        "The public site must work without a native API"
    );
}

#[test]
fn browser_model_command_runs_without_owner_state() {
    let mut previous = None;
    for _ in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("browser-model")
            .env_remove("DOTFILES_VERIFICATION_SOURCE")
            .env_remove("DOTFILES_VERIFICATION_PLAN")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        for key in ["source", "plan"] {
            let digest = report[key].as_str().unwrap();
            assert_eq!(digest.len(), 64);
            assert!(
                digest
                    .bytes()
                    .all(|byte| b"0123456789abcdef".contains(&byte))
            );
        }
        assert_eq!(report["results"][0]["executed"], 54);
        if let Some(previous) = previous {
            assert_eq!(report, previous);
        }
        previous = Some(report);
    }
}

#[test]
fn browser_model_command_reports_the_actual_scope_counts_and_bindings() {
    let source = "a".repeat(40);
    let plan = "b".repeat(64);
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("browser-model")
        .env("DOTFILES_VERIFICATION_SOURCE", &source)
        .env("DOTFILES_VERIFICATION_PLAN", &plan)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["version"], 1);
    assert_eq!(report["method"], "model");
    assert_eq!(report["source"], source);
    assert_eq!(report["plan"], plan);
    assert_eq!(report["scope"], "affected");
    assert_eq!(
        report["results"][0]["name"],
        "browser-store-record-order-and-recovery"
    );
    assert_eq!(report["results"][0]["executed"], 54);
    for key in ["failed", "skipped", "inconclusive"] {
        assert_eq!(report["results"][0][key], 0);
    }
}

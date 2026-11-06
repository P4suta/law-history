use std::{fs, path::Path, process::Command};

fn argument_recorder() -> tempfile::TempDir {
    let test_executable = std::env::current_exe().unwrap();
    let directory = tempfile::Builder::new()
        .prefix("command-contract-")
        .tempdir_in(test_executable.parent().unwrap())
        .unwrap();
    let executable = directory
        .path()
        .join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    let compiled = Command::new("rustc")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/argument_recorder.rs"))
        .arg("--edition=2024")
        .arg("-o")
        .arg(executable)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    directory
}

fn recipe_command(recorder: &Path, recipe: &str, arguments: &[&str]) -> Command {
    let path = std::env::join_paths(
        std::iter::once(recorder.to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut command = Command::new("just");
    command
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .arg(recipe)
        .args(arguments)
        .env("PATH", path)
        .env("LAW_HISTORY_TEST_ARGV_OUTPUT", recorder.join("arguments"));
    command
}

fn image_command(recorder: &Path) -> Command {
    fs::copy(
        recorder.join(format!("cargo{}", std::env::consts::EXE_SUFFIX)),
        recorder.join(format!("docker{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    let path = std::env::join_paths(
        std::iter::once(recorder.to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .args(["mutations", "--list"])
        .env("PATH", path)
        .env("LAW_HISTORY_TEST_ARGV_OUTPUT", recorder.join("arguments"))
        .env("LAW_HISTORY_TEST_ARGV_APPEND", "1")
        .env(
            "LAW_HISTORY_TEST_DOCKER_IMAGE",
            format!("sha256:{}", "0".repeat(64)),
        );
    command
}

#[test]
fn image_builds_inspect_their_own_tags_and_keep_each_job_distinct() {
    let recorder = argument_recorder();
    let mut tags = Vec::new();
    for _ in 0..2 {
        let output = image_command(recorder.path()).output().unwrap();
        assert!(!output.status.success());
        let record = fs::read_to_string(recorder.path().join("arguments")).unwrap();
        let commands: Vec<Vec<_>> = record
            .lines()
            .map(|line| line.split('\0').collect())
            .collect();
        let build = commands
            .iter()
            .find(|arguments| arguments[0] == "build")
            .unwrap();
        let position = build
            .iter()
            .position(|argument| *argument == "--tag")
            .unwrap();
        let tag = build[position + 1];
        let inspection = commands
            .iter()
            .find(|arguments| arguments[0..2] == ["image", "inspect"])
            .unwrap();
        assert!(inspection.contains(&tag));
        assert!(
            !commands
                .iter()
                .any(|arguments| arguments.starts_with(&["image", "rm"]))
        );
        tags.push(tag.to_owned());
        fs::remove_file(recorder.path().join("arguments")).unwrap();
    }
    assert_ne!(tags[0], tags[1], "Build jobs must own distinct image tags");
}

#[test]
fn container_finalization_checks_each_independent_exit_and_memory_verdict() {
    let recorder = argument_recorder();
    for inspected in [0, 17] {
        for removed in [0, 19] {
            for memory in ["false", "true", "invalid"] {
                let output = image_command(recorder.path())
                    .env("LAW_HISTORY_TEST_INSPECTION_EXIT", inspected.to_string())
                    .env("LAW_HISTORY_TEST_REMOVAL_EXIT", removed.to_string())
                    .env("LAW_HISTORY_TEST_OOM_VERDICT", memory)
                    .output()
                    .unwrap();
                assert!(!output.status.success());
                let error = String::from_utf8_lossy(&output.stderr);
                let expected = match (inspected, removed, memory) {
                    (0, 0, "false") => "exit Some(23), resource stop false",
                    (0, 0, "true") => "exit Some(23), resource stop true",
                    (0, 0, _) => "Unexpected container memory verdict",
                    _ => "Cannot inspect or remove the owned verification container",
                };
                assert!(
                    error.contains(expected),
                    "{inspected}/{removed}/{memory}: {error}"
                );
                if inspected != 0 || removed != 0 || memory == "invalid" {
                    assert!(
                        error.contains("requested verification image is unavailable"),
                        "{error}"
                    );
                }
            }
        }
    }
}

#[test]
fn container_finalization_preserves_the_original_verifier_failure() {
    let recorder = argument_recorder();
    let output = image_command(recorder.path())
        .env("LAW_HISTORY_TEST_CONTAINER_MISSING", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("requested verification image is unavailable"),
        "{error}"
    );
    assert!(error.contains("verifier exit=Some(23)"), "{error}");
    assert!(
        error.contains("owned verification container is absent"),
        "{error}"
    );
}

#[test]
fn command_recipes_preserve_arguments_through_the_real_shell() {
    let recorder = argument_recorder();
    for recipe in ["mutations", "mutants"] {
        for arguments in [
            vec!["--matching", "source_inventory|selected", "--list"],
            vec!["--matching", "title with spaces", "--list"],
            vec!["--matching", "$LAW_HISTORY_TEST_LITERAL", "--list"],
            vec!["--matching", "'quoted'", "--list"],
            vec!["--matching", "*", "--list"],
            vec!["--matching", "", "--list"],
            vec!["--matching", "two\nlines", "--list"],
            Vec::new(),
        ] {
            let output = recipe_command(recorder.path(), recipe, &arguments)
                .env("LAW_HISTORY_TEST_LITERAL", "expanded value")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "The {recipe} recipe must preserve {arguments:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let expected = ["xtask", "mutations"]
                .into_iter()
                .chain(arguments.iter().copied())
                .collect::<Vec<_>>()
                .join("\0");
            assert_eq!(
                fs::read_to_string(recorder.path().join("arguments")).unwrap(),
                expected
            );
        }
    }
    let output = recipe_command(recorder.path(), "release-check", &["corpus with spaces"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(recorder.path().join("arguments")).unwrap(),
        ["xtask", "release-check", "--corpus", "corpus with spaces"].join("\0")
    );
}

#[test]
fn command_recipes_propagate_failed_tool_exits() {
    let recorder = argument_recorder();
    for (recipe, arguments) in [
        ("mutants", "--list"),
        ("mutations", "--list"),
        ("release-check", "corpus"),
    ] {
        let output = recipe_command(recorder.path(), recipe, &[arguments])
            .env("LAW_HISTORY_TEST_ARGV_EXIT", "23")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(23));
    }
}

#[test]
fn mutation_campaign_refuses_a_failed_complete_workspace_before_listing_candidates() {
    let recorder = argument_recorder();
    fs::copy(
        recorder
            .path()
            .join(format!("cargo{}", std::env::consts::EXE_SUFFIX)),
        recorder
            .path()
            .join(format!("docker{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    let path = std::env::join_paths(
        std::iter::once(recorder.path().to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "mutations",
            "--image",
            &format!("sha256:{}", "0".repeat(64)),
        ])
        .env("PATH", path)
        .env(
            "LAW_HISTORY_TEST_ARGV_OUTPUT",
            recorder.path().join("arguments"),
        )
        .env("LAW_HISTORY_TEST_ARGV_EXIT", "23")
        .env("LAW_HISTORY_TEST_ARGV_APPEND", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if fs2::available_space(root).unwrap() < 3_221_225_472 {
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("Insufficient free verification storage")
        );
        assert!(!recorder.path().join("arguments").exists());
        return;
    }
    let actual = fs::read_to_string(recorder.path().join("arguments")).unwrap();
    let expected = [
        "cargo",
        "test",
        "--workspace",
        "--locked",
        "--",
        "--test-threads=1",
    ]
    .join("\0");
    assert!(
        actual.lines().next().unwrap().ends_with(&expected),
        "The complete unmutated workspace must pass before any native inventory runs: {actual:?}"
    );
    assert!(!actual.contains("cargo\0mutants\0"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Unmutated workspace baseline failed")
    );
}

#[test]
fn browser_command_builds_static_assets() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("build-browser")
        .output()
        .expect("Run the publication build");
    assert!(
        output.status.success(),
        "The publication command must build static assets: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let index = fs::read_to_string(root.join("web/dist-pages/index.html")).unwrap();
    assert!(index.contains("type=\"module\""));
    let modules: Vec<_> = fs::read_dir(root.join("web/dist-pages/assets"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|suffix| suffix == "wasm"))
        .collect();
    assert_eq!(modules.len(), 1);
    assert!(
        fs::read(&modules[0])
            .unwrap()
            .starts_with(b"\0asm\x01\0\0\0")
    );
}

#[test]
fn capacity_command_matches_independent_allocation_limits() {
    for available in [
        0,
        2_147_483_647,
        2_147_483_648,
        3_221_225_471,
        3_221_225_472,
        4_294_967_295,
        4_294_967_296,
        8_589_934_592,
        u64::MAX,
    ] {
        for requested in [0, 1, 2, 6, 8, usize::MAX] {
            let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
                .args([
                    "mutation-capacity",
                    "--available-bytes",
                    &available.to_string(),
                    "--jobs",
                    &requested.to_string(),
                ])
                .output()
                .expect("Inspect mutation capacity without starting work");
            assert!(
                output.status.success(),
                "The capacity command must report the scheduling decision: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let expected = (1..=6)
                .filter(|count| {
                    *count <= requested
                        && available >= 2_147_483_648 + *count as u64 * 1_073_741_824
                })
                .max()
                .unwrap_or(0);
            assert_eq!(report["workers"].as_u64(), Some(expected as u64));
            assert_eq!(report["available_bytes"].as_u64(), Some(available));
        }
    }
}

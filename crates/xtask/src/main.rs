use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use law_history::{
    Store, api, ingest,
    model::Day,
    reconstruction::{self, OfficialComparison},
    service,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command as Process, Stdio},
    time::{Duration, Instant},
};

mod browser;
mod complete_mutation_inventory;
mod cover_evidence;
mod image_reference;
mod native_log;

const FUZZ_TARGETS: [&str; 6] = [
    "browser_snapshot",
    "law_xml",
    "legal_day",
    "amendment_text",
    "object_codec",
    "reference_resolution",
];
const FUZZ_RUNS: usize = 10_000;
const VERIFICATION_TIMEOUT: Duration = Duration::from_secs(900);
const VERIFIER_POLL_INTERVAL: Duration = Duration::from_millis(100);
const VERIFICATION_IMAGE_RANDOM_BYTES: usize = 16;
const SECRET_REJECTION_EXIT: i32 = 17;
const RUSTSEC_REPOSITORY: &str = "https://github.com/RustSec/advisory-db";
const GIT_COMMIT_HEX_LENGTH: usize = 40;
const MINIMUM_PARSER_CASES: u32 = 1000;
const DEFAULT_PARSER_CASES: u32 = 10000;
const GENERATED_ARTICLE_PERIOD: u32 = 1000;
const GENERATED_REFERENCE_PERIOD: u32 = 999;
const GENERATED_REFERENCE_STRIDE: u32 = 17;
const KANI_BANNER: &str = "Kani Rust Verifier 0.68.0 (cargo plugin)";
const NEGATIVE_PROOF_MESSAGE: &str =
    "The negative control claims that an applicable patch has the wrong base";
const KANI_COUNTEREXAMPLE_EXIT: i32 = 1;

mod mutation;
mod mutation_capacity;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    BuildBrowser,
    PagesServer {
        #[arg(long, default_value_t = 37652)]
        port: u16,
    },
    BrowserModel,
    MutationCapacity {
        #[arg(long)]
        available_bytes: u64,
        #[arg(long)]
        jobs: usize,
    },
    Check,
    Audit,
    Schema,
    ReleaseCheck {
        #[arg(long)]
        corpus: PathBuf,
    },
    BrowserServer {
        #[arg(long, default_value_t = 37651)]
        port: u16,
    },
    Reconcile {
        #[arg(long)]
        corpus: PathBuf,
    },
    ParserCorpus {
        #[arg(long, default_value_t = DEFAULT_PARSER_CASES)]
        cases: u32,
    },
    Proofs {
        #[arg(long)]
        image: Option<String>,
    },
    Fuzz {
        #[arg(long)]
        image: Option<String>,
    },
    #[command(alias = "mutants")]
    Mutations(mutation::Options),
}

#[derive(Deserialize)]
struct Obligations {
    harnesses: Vec<String>,
    required_covers: BTreeMap<String, usize>,
    negative_control: String,
    source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum EvidenceMethod {
    Fuzz,
    Mutation,
}

fn completed_evidence(
    source: &str,
    plan: &str,
    method: EvidenceMethod,
    tool_version: &str,
    target: &str,
    bounds: &str,
    subjects: &[(&str, usize)],
) -> serde_json::Value {
    serde_json::json!({
        "version":1,
        "source":source,
        "plan":plan,
        "method":method,
        "scope":"affected",
        "tool_version":tool_version,
        "target":target,
        "bounds":bounds,
        "results":subjects.iter().map(|(name, executed)| serde_json::json!({
            "name":name,"status":"passed","executed":executed,
            "failed":0,"skipped":0,"inconclusive":0,
        })).collect::<Vec<_>>(),
    })
}

fn emit_completed_evidence(
    source: &str,
    plan: &str,
    method: EvidenceMethod,
    tool_version: &str,
    target: &str,
    bounds: &str,
    subjects: &[(&str, usize)],
) -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string(&completed_evidence(
            &std::env::var("DOTFILES_VERIFICATION_SOURCE").unwrap_or_else(|_| source.into()),
            &std::env::var("DOTFILES_VERIFICATION_PLAN").unwrap_or_else(|_| plan.into()),
            method,
            tool_version,
            target,
            bounds,
            subjects,
        ))?
    );
    Ok(())
}

fn proof_inventory(output: &str, expected: &[String]) -> anyhow::Result<()> {
    let inventory: BTreeSet<_> = expected.iter().map(String::as_str).collect();
    let checked: Vec<_> = output
        .lines()
        .filter_map(|line| {
            line.strip_prefix("Checking harness ")
                .and_then(|line| line.strip_suffix("..."))
        })
        .collect();
    let actual: BTreeSet<_> = checked.iter().copied().collect();
    if expected.is_empty() || inventory.len() != expected.len() {
        bail!("The proof inventory is empty or repeats a harness");
    }
    if !output.lines().any(|line| line == KANI_BANNER) {
        bail!("Unexpected Kani verifier version");
    }
    if actual != inventory || checked.len() != inventory.len() {
        bail!("The exact proof inventory does not match");
    }
    Ok(())
}

fn proof_report(
    output: &str,
    expected: &[String],
) -> anyhow::Result<BTreeMap<String, cover_evidence::CoverCounts>> {
    proof_inventory(output, expected)?;
    let count = expected.len();
    if !output.contains(&format!(
        "Complete - {count} successfully verified harnesses, 0 failures, {count} total."
    )) {
        bail!("The proof report does not establish the complete expected inventory");
    }
    let mut current = None;
    let mut covers = BTreeMap::new();
    for line in output.lines() {
        if let Some(harness) = line
            .strip_prefix("Checking harness ")
            .and_then(|name| name.strip_suffix("..."))
        {
            current = Some(harness);
        }
        if line.contains("cover properties satisfied") {
            let fields: Vec<_> = line.split_whitespace().collect();
            let [
                "**",
                reached,
                "of",
                total,
                "cover",
                "properties",
                "satisfied",
            ] = fields.as_slice()
            else {
                bail!("The cover summary has an unsupported format");
            };
            let counts = cover_evidence::CoverCounts {
                reached: reached.parse()?,
                total: total.parse()?,
            };
            if !cover_evidence::is_complete(Some(counts), None) {
                bail!("A proof cover property has no witness");
            }
            let harness = current.context("A cover summary has no harness")?;
            if covers.insert(harness.to_owned(), counts).is_some() {
                bail!("The proof report repeats a cover summary");
            }
        }
    }
    Ok(covers)
}

fn require_cover_inventory(
    actual: &BTreeMap<String, cover_evidence::CoverCounts>,
    required: &BTreeMap<String, usize>,
) -> anyhow::Result<()> {
    if required.is_empty() {
        bail!("The required cover inventory is empty");
    }
    for (harness, count) in required {
        if !cover_evidence::is_complete(actual.get(harness).copied(), Some(*count)) {
            bail!("The required cover inventory is incomplete for {harness}");
        }
    }
    Ok(())
}

fn negative_proof_report(output: &str, expected: &str) -> anyhow::Result<()> {
    proof_inventory(output, &[expected.to_owned()])?;
    if !output.contains("Complete - 0 successfully verified harnesses, 1 failures, 1 total.")
        || !output.contains(&format!("Verification failed for - {expected}"))
        || !output.contains(NEGATIVE_PROOF_MESSAGE)
        || !output.contains("VERIFICATION:- FAILED")
    {
        bail!("The proof negative control lacks its expected assertion counterexample");
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Verification<'a> {
    Proofs,
    NegativeControl(&'a str),
    Fuzz(&'a str),
}

impl Verification<'_> {
    fn label(&self) -> &str {
        match self {
            Self::Proofs => "proofs",
            Self::NegativeControl(_) => "negative-control",
            Self::Fuzz(target) => target,
        }
    }
}

fn image(root: &Path, supplied: Option<String>) -> anyhow::Result<String> {
    if let Some(image) = supplied {
        if !image_reference::is_immutable(&image) {
            bail!("A supplied verification image needs an immutable digest");
        }
        return Ok(image);
    }
    let identity = tempfile::Builder::new()
        .prefix("build-")
        .rand_bytes(VERIFICATION_IMAGE_RANDOM_BYTES)
        .tempdir()?;
    let tag = format!(
        "law-history-verification:{}",
        identity
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .context("The verification image tag is unavailable")?
    );
    let status = Process::new("docker")
        .current_dir(root)
        .stdout(std::io::stderr())
        .args([
            "build",
            "--tag",
            &tag,
            "--file",
            "verification/Containerfile",
            ".",
        ])
        .status()?;
    if !status.success() {
        bail!("The verification image could not be built: {status}");
    }
    let output = Process::new("docker")
        .args(["image", "inspect", &tag, "--format", "{{.Id}}"])
        .output()?;
    if !output.status.success() {
        bail!("The verification image cannot be inspected");
    }
    let image = String::from_utf8(output.stdout)?.trim().to_owned();
    if !image_reference::is_immutable(&image) {
        bail!("The verification image identity is not immutable");
    }
    Ok(image)
}

fn container(root: &Path, supplied: Option<String>, fuzz: bool) -> anyhow::Result<()> {
    let image = image(root, supplied)?;
    let obligations: Obligations =
        serde_json::from_slice(&fs::read(root.join("verification/obligations.json"))?)?;
    let source_files = source_inventory(root, true, &obligations.source)?;
    let source_hash = law_history::store::sha256(&serde_json::to_vec(&source_files)?);
    let snapshot = snapshot_sources(root, &source_files)?;
    let results = root.join("verification/results");
    fs::create_dir_all(&results)?;
    let path = results.join(if fuzz { "fuzz.json" } else { "proofs.json" });
    let mut record = serde_json::json!({"schema_version":1,"image":image,"source_sha256":source_hash,"source_files":source_files,"complete":false,"exit":null});
    fs::write(&path, serde_json::to_vec_pretty(&record)?)?;
    let artifacts = results.join(format!(
        "{}-{}",
        if fuzz {
            "fuzz-artifacts"
        } else {
            "proof-artifacts"
        },
        std::process::id()
    ));
    fs::create_dir(&artifacts)?;
    record["artifacts"] = serde_json::json!(artifacts.file_name().and_then(|name| name.to_str()));
    fs::write(&path, serde_json::to_vec_pretty(&record)?)?;
    let build_cache = tempfile::tempdir()?;
    let mut output = String::new();
    if fuzz {
        for target in FUZZ_TARGETS {
            let report = verifier_output(
                snapshot.path(),
                &image,
                Verification::Fuzz(target),
                build_cache.path(),
                &artifacts,
            )?;
            if !report.contains(&format!("Done {FUZZ_RUNS} runs")) {
                bail!("Fuzz target {target} did not complete the required fixed run count");
            }
            output.push_str(&format!("Target {target}\n{report}\n"));
        }
    } else {
        output = verifier_output(
            snapshot.path(),
            &image,
            Verification::Proofs,
            build_cache.path(),
            &artifacts,
        )?;
        let covers = proof_report(&output, &obligations.harnesses)?;
        require_cover_inventory(&covers, &obligations.required_covers)?;
        let negative = verifier_output(
            snapshot.path(),
            &image,
            Verification::NegativeControl(&obligations.negative_control),
            build_cache.path(),
            &artifacts,
        )?;
        negative_proof_report(&negative, &obligations.negative_control)?;
        output.push_str(&format!("\nProof negative control\n{negative}"));
    }
    record["output"] = serde_json::json!(output);
    fs::write(&path, serde_json::to_vec_pretty(&record)?)?;
    if source_files != source_inventory(root, true, &obligations.source)?
        || source_files != source_inventory(snapshot.path(), true, &obligations.source)?
    {
        bail!("Source changed during verification");
    }
    fs::create_dir_all(root.join("verification/results"))?;
    record["complete"] = serde_json::json!(true);
    record["exit"] = serde_json::json!(0);
    fs::write(path, serde_json::to_vec_pretty(&record)?)?;
    if fuzz {
        emit_completed_evidence(
            &source_hash,
            &law_history::store::sha256(&fs::read(root.join("verification/obligations.json"))?),
            EvidenceMethod::Fuzz,
            "cargo-fuzz 0.13.2; nightly-2026-10-01",
            &image,
            &format!(
                "{} targets; 10000 runs each; seed 20261105; max_len 65536; ASan, overflow checks and debug assertions; 2 CPUs, 2048 MiB, 256 PIDs, 900 seconds per container",
                FUZZ_TARGETS.len()
            ),
            &FUZZ_TARGETS.map(|target| (target, FUZZ_RUNS)),
        )?;
    } else {
        println!(
            "Verified all {} production policy harnesses with their reachable outcomes, and rejected all 1 counterexamples",
            obligations.harnesses.len()
        );
    }
    Ok(())
}

fn verifier_output(
    root: &Path,
    image: &str,
    mode: Verification<'_>,
    build_cache: &Path,
    artifacts: &Path,
) -> anyhow::Result<String> {
    let name = format!("law-history-{}-{}", mode.label(), std::process::id());
    let mount = format!("type=bind,source={},target=/work,readonly", root.display());
    let build_mount = format!(
        "type=bind,source={},target=/tmp/law-history-verification",
        build_cache.display()
    );
    fs::create_dir_all(root.join("fuzz/generated"))?;
    fs::create_dir_all(root.join("fuzz/artifacts"))?;
    let artifacts_mount = format!(
        "type=bind,source={},target=/work/fuzz/artifacts",
        artifacts.display()
    );
    let generated = tempfile::tempdir()?;
    let generated_mount = format!(
        "type=bind,source={},target=/work/fuzz/generated",
        generated.path().display()
    );
    let mut command = Process::new("docker");
    command.args([
        "run",
        "--init",
        "--name",
        &name,
        "--memory",
        "2048m",
        "--memory-swap",
        "2048m",
        "--cpus",
        "2",
        "--pids-limit",
        "256",
        "--mount",
        &mount,
        "--mount",
        &build_mount,
        "--workdir",
        "/work",
        "--env",
        "CARGO_TARGET_DIR=/tmp/law-history-verification",
        "--env",
        "CARGO_BUILD_JOBS=2",
    ]);
    if matches!(mode, Verification::Fuzz(_)) {
        command.args(["--mount", &generated_mount, "--mount", &artifacts_mount]);
    }
    command.arg(image);
    if let Verification::Fuzz(target) = mode {
        command.args([
            "cargo",
            "+nightly-2026-10-01",
            "fuzz",
            "run",
            "--codegen-units=16",
            target,
            "fuzz/generated",
            &format!("fuzz/corpus/{target}"),
            "--",
            &format!("-runs={FUZZ_RUNS}"),
            "-seed=20261105",
            "-max_len=65536",
            "-rss_limit_mb=1536",
        ]);
    } else {
        command.args([
            "cargo",
            "kani",
            "--package",
            "law-domain",
            "--output-format",
            "terse",
        ]);
        if let Verification::NegativeControl(harness) = mode {
            command.args(["--features", "proof-negative-control", "--harness", harness]);
        }
    }
    let result = bounded_output(&mut command, &name)?;
    let label = mode.label();
    fs::write(
        artifacts.join(format!("{label}.stdout.log")),
        &result.stdout,
    )?;
    fs::write(
        artifacts.join(format!("{label}.stderr.log")),
        &result.stderr,
    )?;
    fs::write(
        artifacts.join(format!("{label}.process.json")),
        serde_json::to_vec_pretty(&serde_json::json!({
            "exit":result.code,"timed_out":result.timed_out,"memory_exceeded":result.memory_exceeded
        }))?,
    )?;
    let output = format!("{}\n{}", result.stdout, result.stderr);
    eprintln!("{output}");
    if result.timed_out {
        bail!("Verification is inconclusive after the 900-second resource limit");
    }
    if result.memory_exceeded {
        bail!("Verification is inconclusive after exceeding the container memory limit");
    }
    let expected_exit = if matches!(mode, Verification::NegativeControl(_)) {
        KANI_COUNTEREXAMPLE_EXIT
    } else {
        0
    };
    if result.code != Some(expected_exit) {
        bail!(
            "Unexpected verifier exit ({:?}); inspect the retained native output",
            result.code
        );
    }
    Ok(output)
}

struct Captured {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    memory_exceeded: bool,
}

struct OwnedContainer<'a> {
    name: &'a str,
    removed: bool,
}

fn container_removal(name: &str) -> Process {
    let mut command = Process::new("docker");
    command.args(["rm", "--force", "--volumes", name]);
    command
}

impl Drop for OwnedContainer<'_> {
    fn drop(&mut self) {
        if !self.removed {
            match container_removal(self.name).output() {
                Ok(output) if output.status.success() => {}
                Ok(output) => eprintln!(
                    "Container cleanup failed for {}: {}",
                    self.name,
                    String::from_utf8_lossy(&output.stderr)
                ),
                Err(error) => eprintln!("Container cleanup failed for {}: {error}", self.name),
            }
        }
    }
}

fn stop_verifier(command: &mut Process) -> anyhow::Result<()> {
    let stopped = command
        .status()
        .context("Cannot stop the owned verification container")?;
    if !stopped.success() {
        bail!("The owned verification container could not be stopped: {stopped}");
    }
    Ok(())
}

fn bounded_output(command: &mut Process, name: &str) -> anyhow::Result<Captured> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let mut container = OwnedContainer {
        name,
        removed: false,
    };
    let mut stdout = child
        .stdout
        .take()
        .context("Verifier stdout is unavailable")?;
    let mut stderr = child
        .stderr
        .take()
        .context("Verifier stderr is unavailable")?;
    let output_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let error_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= VERIFICATION_TIMEOUT {
            timed_out = true;
            let mut stop = Process::new("docker");
            stop.args(["stop", "--timeout", "5", name]);
            stop_verifier(&mut stop)?;
            break child.wait()?;
        }
        std::thread::sleep(VERIFIER_POLL_INTERVAL);
    };
    let stdout = output_reader
        .join()
        .map_err(|_| anyhow::anyhow!("Verifier output reader failed"))??;
    let stderr = error_reader
        .join()
        .map_err(|_| anyhow::anyhow!("Verifier error reader failed"))??;
    let verifier = format!(
        "verifier exit={:?}\nstdout:\n{}\nstderr:\n{}",
        status.code(),
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    let inspected = Process::new("docker")
        .args(["inspect", "--format", "{{.State.OOMKilled}}", name])
        .output()
        .with_context(|| format!("Cannot inspect the owned verification container; {verifier}"))?;
    let removed = container_removal(name)
        .output()
        .with_context(|| format!("Cannot remove the owned verification container; {verifier}"))?;
    container.removed = removed.status.success();
    if !inspected.status.success() || !removed.status.success() {
        bail!(
            "Cannot inspect or remove the owned verification container; {verifier}\ninspection exit={:?}: {}\nremoval exit={:?}: {}",
            inspected.status.code(),
            String::from_utf8_lossy(&inspected.stderr),
            removed.status.code(),
            String::from_utf8_lossy(&removed.stderr)
        );
    }
    let state = String::from_utf8_lossy(&inspected.stdout);
    let memory_exceeded = match state.trim() {
        "true" => true,
        "false" => false,
        _ => bail!("Unexpected container memory verdict {state:?}; {verifier}"),
    };
    Ok(Captured {
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        timed_out,
        memory_exceeded,
    })
}

fn source_inventory(
    root: &Path,
    fuzz: bool,
    proof_source: &str,
) -> anyhow::Result<BTreeMap<String, String>> {
    fn visit(
        root: &Path,
        path: &Path,
        output: &mut BTreeMap<String, String>,
        source_only: bool,
    ) -> anyhow::Result<()> {
        for entry in fs::read_dir(path)? {
            let path = entry?.path();
            if path == root.join("web/src/wasm") {
                continue;
            }
            if path.is_dir() {
                visit(root, &path, output, source_only)?;
            } else if !source_only
                || path.extension().is_some_and(|extension| extension == "rs")
                || path.file_name().is_some_and(|name| name == "Cargo.toml")
            {
                output.insert(
                    path.strip_prefix(root)?.to_string_lossy().into_owned(),
                    law_history::store::sha256(&fs::read(path)?),
                );
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    visit(root, &root.join("crates"), &mut files, true)?;
    for path in [
        proof_source,
        "Cargo.toml",
        "Cargo.lock",
        "crates/law-domain/Cargo.toml",
        "crates/law-history/Cargo.toml",
        "crates/law-history-cli/Cargo.toml",
        "crates/xtask/Cargo.toml",
        ".cargo/config.toml",
        ".cargo/mutants.toml",
        ".gitignore",
        "justfile",
        "mise.toml",
        "rust-toolchain.toml",
        "verification/obligations.json",
        "verification/Containerfile",
        "crates/xtask/src/main.rs",
        "crates/xtask/src/mutation.rs",
    ] {
        files.insert(
            path.into(),
            law_history::store::sha256(&fs::read(root.join(path))?),
        );
    }
    if fuzz {
        visit(root, &root.join("web/src"), &mut files, false)?;
        visit(root, &root.join("web/pages-tests"), &mut files, false)?;
        visit(root, &root.join("web/tools"), &mut files, false)?;
        for path in [
            "web/package.json",
            "web/package-lock.json",
            "web/tsconfig.json",
            "web/vite.config.ts",
            "web/playwright.pages.config.ts",
            "web/biome.json",
            "web/index.html",
            "web/openapi.json",
        ] {
            files.insert(
                path.into(),
                law_history::store::sha256(&fs::read(root.join(path))?),
            );
        }
        visit(root, &root.join("crates/law-history/src"), &mut files, true)?;
        visit(
            root,
            &root.join("crates/law-history/tests"),
            &mut files,
            false,
        )?;
        visit(
            root,
            &root.join("crates/law-history-cli/src"),
            &mut files,
            true,
        )?;
        visit(
            root,
            &root.join("crates/law-history-cli/tests"),
            &mut files,
            true,
        )?;
        visit(root, &root.join("fixtures"), &mut files, false)?;
        visit(root, &root.join("fuzz/fuzz_targets"), &mut files, true)?;
        visit(root, &root.join("fuzz/corpus"), &mut files, false)?;
        for path in ["fuzz/Cargo.toml", "fuzz/Cargo.lock"] {
            files.insert(
                path.into(),
                law_history::store::sha256(&fs::read(root.join(path))?),
            );
        }
    }
    Ok(files)
}

fn snapshot_sources(
    root: &Path,
    files: &BTreeMap<String, String>,
) -> anyhow::Result<tempfile::TempDir> {
    let snapshot = tempfile::tempdir()?;
    if files.is_empty() {
        bail!("A source snapshot requires a nonempty inventory");
    }
    for (path, digest) in files {
        if !Path::new(path)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            bail!("The source inventory contains an unsafe path");
        }
        let destination = snapshot.path().join(path);
        fs::create_dir_all(
            destination
                .parent()
                .context("Snapshot file has no parent")?,
        )?;
        let bytes = fs::read(root.join(path))?;
        if &law_history::store::sha256(&bytes) != digest {
            bail!("Source changed while preparing the immutable snapshot");
        }
        fs::write(destination, bytes)?;
    }
    Ok(snapshot)
}

fn root() -> anyhow::Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?)
}

fn run(directory: &Path, program: &str, arguments: &[&str]) -> anyhow::Result<()> {
    eprintln!("check: {program} {}", arguments.join(" "));
    let status = Process::new(program)
        .args(arguments)
        .current_dir(directory)
        .status()
        .with_context(|| format!("Cannot start {program}"))?;
    if !status.success() {
        bail!("{program} failed with {status}");
    }
    Ok(())
}

fn schema(root: &Path, write: bool) -> anyhow::Result<()> {
    let expected = serde_json::to_vec_pretty(&api::openapi())?;
    let spec = root.join("web/openapi.json");
    if write {
        fs::write(spec, &expected)?;
        run(&root.join("web"), "npm", &["run", "generate"])?;
    } else {
        if fs::read(&spec)? != expected {
            bail!("OpenAPI is stale; run mise x -- just schema");
        }
        let temp = tempfile::tempdir()?;
        let generated = temp.path().join("api.generated.ts");
        run(
            &root.join("web"),
            "node",
            &[
                "tools/generate-api.ts",
                "--output",
                generated.to_str().context("Non-UTF-8 schema path")?,
            ],
        )?;
        if fs::read(generated)? != fs::read(root.join("web/src/api.generated.ts"))? {
            bail!("Generated TypeScript is stale; run mise x -- just schema");
        }
    }
    Ok(())
}

fn check(root: &Path) -> anyhow::Result<()> {
    secret_gate_contract(root)?;
    run(
        root,
        "gitleaks",
        &[
            "dir",
            "--no-banner",
            "--redact",
            "--config",
            ".gitleaks.toml",
            ".",
        ],
    )?;
    run(root, "cargo", &["fmt", "--all", "--", "--check"])?;
    run(
        root,
        "cargo",
        &[
            "fmt",
            "--manifest-path",
            "fuzz/Cargo.toml",
            "--all",
            "--",
            "--check",
        ],
    )?;
    run(
        root,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(root, "cargo", &["test", "--workspace", "--locked"])?;
    run(
        root,
        "cargo",
        &[
            "clippy",
            "--manifest-path",
            "fuzz/Cargo.toml",
            "--bins",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    schema(root, false)?;
    run(&root.join("web"), "npm", &["run", "test:contracts"])?;
    run(&root.join("web"), "npm", &["run", "lint"])?;
    run(&root.join("web"), "npm", &["run", "build"])?;
    run(&root.join("web"), "npm", &["test"])?;
    println!(
        "Required checks passed: Rust lint and regressions, generated API contracts, TypeScript, and browser workflows"
    );
    Ok(())
}

fn secret_gate_contract(root: &Path) -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let report = directory.path().join("secrets.json");
    let config = root.join(".gitleaks.toml");
    let mut scanner = Process::new("gitleaks")
        .args([
            "stdin",
            "--no-banner",
            "--redact",
            "--exit-code",
            &SECRET_REJECTION_EXIT.to_string(),
            "--report-format",
            "json",
            "--report-path",
        ])
        .arg(&report)
        .arg("--config")
        .arg(config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let dummy = ["ghp_", "r7N9kQ2mX8pL4vS6dF1h", "J5cT0bW3yZ9uG2eA"].concat();
    writeln!(
        scanner
            .stdin
            .take()
            .context("Secret scanner input is unavailable")?,
        "token = \"{dummy}\""
    )?;
    let result = scanner.wait_with_output()?;
    if result.status.code() != Some(SECRET_REJECTION_EXIT) {
        bail!(
            "The secret scan gate did not reject the planted dummy token: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let report: serde_json::Value = serde_json::from_slice(&fs::read(report)?)?;
    if !report.as_array().is_some_and(|findings| {
        findings
            .iter()
            .any(|finding| finding["RuleID"] == "github-pat")
    }) {
        bail!("The secret scan gate reported a different failure than the planted token");
    }
    println!("Secret scan negative control passed");
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdvisorySnapshot {
    schema_version: u32,
    repository: String,
    revision: String,
    checked_date: String,
}

fn audit(root: &Path) -> anyhow::Result<()> {
    let snapshot: AdvisorySnapshot =
        serde_json::from_slice(&fs::read(root.join("verification/advisories.json"))?)?;
    if snapshot.schema_version != 1
        || snapshot.repository != RUSTSEC_REPOSITORY
        || snapshot.revision.len() != GIT_COMMIT_HEX_LENGTH
        || !snapshot
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("The advisory snapshot identity is invalid");
    }
    let directory = tempfile::tempdir()?;
    run(directory.path(), "git", &["init", "--quiet"])?;
    run(
        directory.path(),
        "git",
        &[
            "fetch",
            "--quiet",
            "--depth",
            "1",
            &snapshot.repository,
            &snapshot.revision,
        ],
    )?;
    run(
        directory.path(),
        "git",
        &["checkout", "--quiet", "--detach", "FETCH_HEAD"],
    )?;
    run(
        root,
        "cargo",
        &[
            "audit",
            "--no-fetch",
            "--db",
            directory
                .path()
                .to_str()
                .context("Non-UTF-8 advisory path")?,
            "--file",
            "Cargo.lock",
        ],
    )?;
    run(&root.join("web"), "npm", &["audit", "--audit-level=low"])?;
    println!(
        "Dependency audit passed against RustSec {} ({}) and the live npm advisory service",
        snapshot.revision, snapshot.checked_date
    );
    Ok(())
}

fn reconcile(root: &Path, corpus: &Path) -> anyhow::Result<()> {
    let store = Store::open(corpus)?;
    store.verify_integrity()?;
    let status = service::status(&store, None)?;
    if !status.diagnostics.is_empty() {
        bail!("Recorded official amendment mismatches remain unresolved");
    }
    let catalog = status
        .catalog
        .context("Full reconciliation requires a preserved official catalog")?;
    if !catalog.complete || catalog.law_ids.is_empty() {
        bail!("The source catalog is incomplete or empty");
    }
    let mut required = BTreeSet::new();
    for law in &catalog.law_ids {
        let coverage = service::coverage(&store, law, None)?;
        if !coverage.all_available_stored || !coverage.diagnostics.is_empty() {
            bail!("Source coverage is incomplete for {law}");
        }
        let mut latest = BTreeMap::<String, law_history::StoredRevision>::new();
        for revision in store.history(law, None)? {
            let id = revision.revision.revision_id.clone();
            if latest
                .get(&id)
                .is_none_or(|previous| previous.observed_at < revision.observed_at)
            {
                latest.insert(id, revision);
            }
        }
        let mut ordered: Vec<_> = latest.into_values().collect();
        ordered.sort_by_key(|item| {
            (
                item.revision.effective.day().is_none(),
                item.revision.effective.day(),
                item.revision.sequence,
                item.revision.revision_id.clone(),
            )
        });
        for revision in ordered.into_iter().skip(1) {
            if revision.revision.source.confidence == law_history::Confidence::Official {
                required.insert(revision.version);
            }
        }
    }
    let records: Vec<OfficialComparison> = store.facts_with_prefix("comparison:", None)?;
    if records.is_empty() {
        bail!("Official amendment comparisons are absent");
    }
    let mut matched = BTreeSet::new();
    for item in records {
        let result = reconstruction::compare(&store, &item)?;
        if !result.diagnostics.is_empty() {
            bail!("Amendment comparison was refused: {:?}", result.diagnostics);
        }
        matched.extend(result.matched_versions);
    }
    if let Some(missing) = required.difference(&matched).next() {
        bail!("Official transition lacks a passing comparison: {missing}");
    }
    let compared = matched.len();
    let report = serde_json::json!({"catalog_laws":catalog.law_ids.len(),"official_comparisons":compared,"objects":store.verify_integrity()?});
    fs::create_dir_all(root.join("verification/results"))?;
    fs::write(
        root.join("verification/results/reconciliation.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("Reconciled {compared} official amendment results");
    Ok(())
}

fn parser_corpus(cases: u32) -> anyhow::Result<()> {
    if cases < MINIMUM_PARSER_CASES {
        bail!("The parser campaign requires at least 1000 deterministic cases");
    }
    for index in 0..cases {
        let xml = format!(
            "<Law><LawBody><MainProvision><Article Num=\"{}\"><Paragraph Num=\"1\"><Sentence>第{}条の申請&lt;{}&amp;保存</Sentence></Paragraph></Article></MainProvision></LawBody></Law>",
            index % GENERATED_ARTICLE_PERIOD + 1,
            (index.wrapping_mul(GENERATED_REFERENCE_STRIDE)) % GENERATED_REFERENCE_PERIOD + 1,
            index
        );
        let tree = law_history::xml::parse_xml(&xml).map_err(anyhow::Error::msg)?;
        let encoded = serde_json::to_vec(&tree)?;
        let decoded = serde_json::from_slice(&encoded)?;
        if tree != decoded {
            bail!("Parser round-trip mismatch at case {index}");
        }
        if law_history::xml::parse_xml(&xml[..xml.len() - 1]).is_ok() {
            bail!("Malformed XML accepted at case {index}");
        }
    }
    println!("Parser corpus passed: {cases} valid inputs and {cases} malformed controls");
    Ok(())
}

fn browser_server(root: &Path, port: u16) -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    ingest::import_bundle(&store, &fs::read(root.join("fixtures/demo.json"))?)?;
    let state = api::ApiState {
        root: directory.path().to_path_buf(),
        today: Some("2026-11-05".parse::<Day>().map_err(anyhow::Error::msg)?),
    };
    let router = api::router(state, Some(root.join("web/dist")));
    tokio::runtime::Runtime::new()?.block_on(async move {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        api::serve(listener, router).await
    })
}

fn main() -> anyhow::Result<()> {
    let root = root()?;
    match Cli::parse().command {
        Command::Check => check(&root),
        Command::BuildBrowser => browser::build(&root),
        Command::PagesServer { port } => browser::serve(&root, port),
        Command::BrowserModel => browser::model(&root),
        Command::MutationCapacity {
            available_bytes,
            jobs,
        } => {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "available_bytes":available_bytes,
                    "requested_workers":jobs,
                    "workers":mutation_capacity::workers(available_bytes, jobs),
                    "reserve_bytes":mutation_capacity::RESERVE_BYTES,
                    "per_worker_bytes":mutation_capacity::WORKER_BYTES
                }))?
            );
            Ok(())
        }
        Command::Audit => audit(&root),
        Command::Schema => schema(&root, true),
        Command::BrowserServer { port } => browser_server(&root, port),
        Command::Reconcile { corpus } => reconcile(&root, &corpus),
        Command::ParserCorpus { cases } => parser_corpus(cases),
        Command::Proofs { image } => container(&root, image, false),
        Command::Fuzz { image } => container(&root, image, true),
        Command::Mutations(options) => mutation::suite(&root, options),
        Command::ReleaseCheck { corpus } => {
            check(&root)?;
            audit(&root)?;
            reconcile(&root, &corpus)?;
            parser_corpus(DEFAULT_PARSER_CASES)?;
            container(&root, None, false)?;
            container(&root, None, true)?;
            mutation::release(&root)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::proof_report;
    use clap::Parser;

    #[test]
    fn verifier_stop_requires_a_successful_process_and_preserves_launch_errors() {
        for (argument, succeeds) in [("--version", true), ("--invalid-stop-probe", false)] {
            let mut command = super::Process::new(env!("CARGO"));
            command
                .arg(argument)
                .stdout(super::Stdio::null())
                .stderr(super::Stdio::null());
            assert_eq!(super::stop_verifier(&mut command).is_ok(), succeeds);
        }
        let root = tempfile::tempdir().unwrap();
        let mut missing = super::Process::new(root.path().join("missing"));
        let error = super::stop_verifier(&mut missing).unwrap_err();
        assert!(format!("{error:#}").contains("Cannot stop the owned verification container"));
    }

    #[test]
    fn mutation_name_filters_are_available_for_native_diagnostic_controls() {
        assert!(
            super::Cli::try_parse_from([
                "xtask",
                "mutants",
                "--scope",
                "tooling",
                "--matching",
                "validate_native_logs"
            ])
            .is_ok()
        );
    }

    #[test]
    fn owned_container_cleanup_removes_only_the_named_container_and_its_anonymous_volumes() {
        for name in ["owned", "law-history-mutations-1-0", "name with spaces"] {
            let command = super::container_removal(name);
            assert_eq!(command.get_program(), "docker");
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                ["rm", "--force", "--volumes", name]
            );
        }
    }

    #[test]
    fn image_digest_bytes_match_an_independent_alphabet_at_every_position_and_length_boundary() {
        for length in 0..=66 {
            let digest = vec![b'a'; length];
            assert_eq!(super::image_reference::valid_digest(&digest), length == 64);
        }
        for position in 0..64 {
            for byte in u8::MIN..=u8::MAX {
                let mut digest = [b'a'; 64];
                digest[position] = byte;
                assert_eq!(
                    super::image_reference::valid_digest(&digest),
                    b"0123456789abcdef".contains(&byte)
                );
                if let Ok(digest) = std::str::from_utf8(&digest) {
                    for prefix in ["sha256:", "name@sha256:", "sha256:tag@sha256:"] {
                        assert_eq!(
                            super::image_reference::is_immutable(&format!("{prefix}{digest}")),
                            b"0123456789abcdef".contains(&byte)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn verification_images_accept_built_ids_and_registry_digests_across_worker_handoffs() {
        let root = tempfile::tempdir().unwrap();
        let digest = "a".repeat(64);
        for reference in [
            format!("sha256:{digest}"),
            format!("law-history-verification@sha256:{digest}"),
            format!("localhost:5000/project/image:tag@sha256:{digest}"),
            format!("sha256:tag@sha256:{digest}"),
        ] {
            let selected = super::image(root.path(), Some(reference.clone())).unwrap();
            assert_eq!(selected, reference);
            assert_eq!(
                super::image(root.path(), Some(selected)).unwrap(),
                reference
            );
        }
    }

    #[test]
    fn verification_images_refuse_tags_missing_names_and_malformed_digests() {
        let root = tempfile::tempdir().unwrap();
        let digest = "a".repeat(64);
        let invalid = [
            String::new(),
            "latest".into(),
            "law-history-verification:latest".into(),
            format!("@sha256:{digest}"),
            format!("name@@sha256:{digest}"),
            "sha256:".into(),
            "name@sha256:123".into(),
            format!("sha256:{}", "a".repeat(63)),
            format!("name@sha256:{}", "a".repeat(65)),
            format!("name@sha256:{}", "A".repeat(64)),
            format!("name@sha256:{digest}\n"),
            format!("name@sha256:{digest}@sha256:{digest}"),
            format!("sha512:{digest}"),
        ];
        for reference in invalid {
            assert!(
                super::image(root.path(), Some(reference.clone())).is_err(),
                "{reference:?}"
            );
        }
    }

    #[test]
    fn completed_evidence_preserves_bindings_native_counts_and_verifier_limits() {
        assert_eq!(super::FUZZ_RUNS, 10_000);
        for (method, name) in [
            (super::EvidenceMethod::Fuzz, "fuzz"),
            (super::EvidenceMethod::Mutation, "mutation"),
        ] {
            let report = super::completed_evidence(
                "source digest",
                "plan digest",
                method,
                "pinned verifier",
                "container digest",
                "fixed count and resource bounds",
                &[
                    ("first native inventory", 123),
                    ("second native inventory", 17),
                ],
            );
            assert_eq!(report["source"], "source digest");
            assert_eq!(report["plan"], "plan digest");
            assert_eq!(report["method"], name);
            assert_eq!(report["scope"], "affected");
            assert_eq!(report["tool_version"], "pinned verifier");
            assert_eq!(report["target"], "container digest");
            assert_eq!(report["bounds"], "fixed count and resource bounds");
            let subjects = report["results"].as_array().unwrap();
            assert_eq!(subjects.len(), 2);
            for (subject, (identity, count)) in subjects.iter().zip([
                ("first native inventory", 123),
                ("second native inventory", 17),
            ]) {
                assert_eq!(subject["name"], identity);
                assert_eq!(subject["executed"], count);
                assert_eq!(subject["status"], "passed");
                assert_eq!(subject["failed"], 0);
                assert_eq!(subject["skipped"], 0);
                assert_eq!(subject["inconclusive"], 0);
            }
        }
    }

    #[test]
    fn proof_gate_refuses_unmet_cover_properties() {
        let expected = vec!["proofs::contract".to_string()];
        for count in ["0 of 6", "5 of 6", "7 of 6", "invalid of 6", "0 of 0"] {
            let report = format!(
                "Kani Rust Verifier 0.68.0 (cargo plugin)\nChecking harness proofs::contract...\n ** {count} cover properties satisfied\nComplete - 1 successfully verified harnesses, 0 failures, 1 total."
            );
            assert!(proof_report(&report, &expected).is_err(), "{count}");
        }
        let valid = "Kani Rust Verifier 0.68.0 (cargo plugin)\nChecking harness proofs::contract...\n ** 6 of 6 cover properties satisfied\nComplete - 1 successfully verified harnesses, 0 failures, 1 total.";
        let actual = proof_report(valid, &expected).unwrap();
        let required = std::collections::BTreeMap::from([("proofs::contract".into(), 6)]);
        assert!(super::require_cover_inventory(&actual, &required).is_ok());
        assert!(
            super::require_cover_inventory(&std::collections::BTreeMap::new(), &required).is_err()
        );
        assert!(
            super::require_cover_inventory(&actual, &std::collections::BTreeMap::new()).is_err()
        );
        for count in [0, 5, 7] {
            assert!(
                super::require_cover_inventory(
                    &actual,
                    &std::collections::BTreeMap::from([("proofs::contract".into(), count)])
                )
                .is_err()
            );
        }
        assert!(proof_report(&valid.replace(" ** 6", " ** extra 6"), &expected).is_err());
        assert!(
            proof_report(
                &valid.replace(
                    " ** 6 of 6 cover properties satisfied",
                    " ** 6 of 6 cover properties satisfied\n ** 6 of 6 cover properties satisfied"
                ),
                &expected
            )
            .is_err()
        );
    }

    #[test]
    fn proof_gate_refuses_missing_failed_and_empty_inventories() {
        let expected = vec!["proofs::contract".to_string()];
        let valid = "Kani Rust Verifier 0.68.0 (cargo plugin)\nChecking harness proofs::contract...\nComplete - 1 successfully verified harnesses, 0 failures, 1 total.";
        assert!(proof_report(valid, &expected).is_ok());
        assert!(
            proof_report(
                "Kani Rust Verifier 0.68.0 (cargo plugin)\nComplete - 0 successfully verified harnesses, 0 failures, 0 total.",
                &expected
            )
            .is_err()
        );
        let failed = proof_report("Kani Rust Verifier 0.68.0 (cargo plugin)\nChecking harness proofs::contract...\nComplete - 0 successfully verified harnesses, 1 failures, 1 total.", &expected).unwrap_err();
        assert!(failed.to_string().contains("does not establish"));
        assert!(proof_report(valid, &[]).is_err());
        assert!(
            proof_report(
                valid,
                &["proofs::contract".into(), "proofs::contract".into()]
            )
            .is_err()
        );
        assert!(proof_report(&valid.replace("0.68.0", "different"), &expected).is_err());
        assert!(
            proof_report(
                &format!("{valid}\nChecking harness proofs::unexpected..."),
                &expected
            )
            .is_err()
        );
    }

    #[test]
    fn snapshots_refuse_empty_stale_unsafe_and_missing_source_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("source.rs"), "original").unwrap();
        let files = std::collections::BTreeMap::from([(
            "source.rs".into(),
            law_history::store::sha256(b"original"),
        )]);
        let snapshot = super::snapshot_sources(root.path(), &files).unwrap();
        std::fs::write(root.path().join("source.rs"), "changed").unwrap();
        assert_eq!(
            std::fs::read(snapshot.path().join("source.rs")).unwrap(),
            b"original"
        );
        assert!(super::snapshot_sources(root.path(), &files).is_err());
        assert!(super::snapshot_sources(root.path(), &std::collections::BTreeMap::new()).is_err());
        assert!(
            super::snapshot_sources(
                root.path(),
                &std::collections::BTreeMap::from([("../source.rs".into(), String::new())])
            )
            .is_err()
        );
        std::fs::remove_file(root.path().join("source.rs")).unwrap();
        assert!(super::snapshot_sources(root.path(), &files).is_err());
    }

    #[test]
    fn assurance_inventory_binds_every_workspace_crate_and_the_browser_tests() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = super::source_inventory(&root, true, "crates/law-domain/src/lib.rs").unwrap();
        for path in [
            "justfile",
            "crates/law-browser/src/lib.rs",
            "crates/law-browser/Cargo.toml",
            "crates/xtask/tests/browser_workspace.rs",
            "web/pages-tests/published.spec.ts",
            "web/src/history.worker.ts",
            "web/package-lock.json",
        ] {
            assert!(
                files.contains_key(path),
                "Missing assurance dependency: {path}"
            );
        }
        assert!(!files.keys().any(|path| path.starts_with("web/src/wasm/")));
        let fixture = super::snapshot_sources(&root, &files).unwrap();
        for (path, bytes) in [
            ("web/src/wasm/generated.rs", "generated"),
            ("crates/discovered-package/Cargo.toml", "[package]"),
            (
                "crates/discovered-package/src/lib.rs",
                "pub fn discovered() {}",
            ),
            (
                "crates/discovered-package/tests/behavior.rs",
                "#[test] fn behavior() {}",
            ),
        ] {
            let file = fixture.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, bytes).unwrap();
        }
        let inventory =
            super::source_inventory(fixture.path(), true, "crates/law-domain/src/lib.rs").unwrap();
        assert!(
            !inventory
                .keys()
                .any(|path| path.starts_with("web/src/wasm/"))
        );
        for path in [
            "crates/discovered-package/Cargo.toml",
            "crates/discovered-package/src/lib.rs",
            "crates/discovered-package/tests/behavior.rs",
        ] {
            assert!(inventory.contains_key(path));
        }
    }

    #[test]
    fn proof_negative_control_requires_the_exact_assertion_and_counterexample() {
        let name = "proofs::negative";
        let valid = format!(
            "{}\nChecking harness {name}...\nFailed Checks: {}\nVERIFICATION:- FAILED\nVerification failed for - {name}\nComplete - 0 successfully verified harnesses, 1 failures, 1 total.",
            super::KANI_BANNER,
            super::NEGATIVE_PROOF_MESSAGE
        );
        assert!(super::negative_proof_report(&valid, name).is_ok());
        for broken in [
            valid.replace(super::NEGATIVE_PROOF_MESSAGE, "Unwinding assertion failure"),
            valid.replace("1 failures", "0 failures"),
            valid.replace("0.68.0", "different"),
            valid.replace("VERIFICATION:- FAILED", "VERIFICATION:- SUCCESSFUL"),
            format!("{valid}\nChecking harness proofs::unexpected..."),
        ] {
            assert!(super::negative_proof_report(&broken, name).is_err());
        }
        assert!(super::negative_proof_report(&valid, "proofs::missing").is_err());
    }
}

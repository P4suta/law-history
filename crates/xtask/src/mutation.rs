use super::{
    Captured, EvidenceMethod, bounded_output, emit_completed_evidence, image, native_log,
    snapshot_sources, source_inventory,
};
use anyhow::{Context, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

const MUTANTS_VERSION: &str = "27.1.0";
const MUTANT_TEST_TIMEOUT_SECONDS: &str = "60";
const MUTANT_BUILD_TIMEOUT_SECONDS: &str = "300";
const MAXIMUM_MUTATION_WORKERS: usize = 6;
const BUILD_AND_TEST_PHASES: usize = 2;
const CARGO_FAILURE_EXIT: i32 = 101;
const CONTAINER_MEMORY_MIB: u32 = 2048;
const CONTAINER_CPUS: u32 = 2;
const CONTAINER_PIDS: u32 = 256;
const CONTAINER_TIMEOUT_SECONDS: u32 = 900;
const SCRATCH_MEMORY_MIB: u32 = 512;
const TEST_TIMEOUT_SECONDS: u32 = 60;
const BUILD_TIMEOUT_SECONDS: u32 = 300;
const LOG_PREVIEW_LINES: usize = 12;
const INFRASTRUCTURE_MARKERS: [&str; 10] = [
    "signal:",
    "SIGKILL",
    "out of memory",
    "No space left on device",
    "Cannot allocate memory",
    "failed to download",
    "failed to get `",
    "INCONCLUSIVE:",
    "internal compiler error",
    "compiler unexpectedly panicked",
];
const RELEASE_MUTATION_SHARDS: usize = 32;
const IMAGE_REFERENCE_MUTATION_SHARDS: usize = 4;
const NATIVE_LOG_MUTATION_SHARDS: usize = 2;
const NATIVE_DIAGNOSTIC_MUTATION_SHARDS: usize = 4;
static RUN_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Scope {
    Domain,
    Runtime,
    Tooling,
}

#[derive(clap::Args)]
pub struct Options {
    #[arg(long)]
    image: Option<String>,
    #[arg(long, value_enum, default_value_t = Scope::Runtime)]
    scope: Scope,
    #[arg(long)]
    file: Option<String>,
    #[arg(long)]
    matching: Option<String>,
    #[arg(long)]
    shard: Option<String>,
    #[arg(long)]
    list: bool,
    #[arg(long, default_value_t = 1)]
    shards: usize,
    #[arg(long, default_value_t = 1)]
    jobs: usize,
}

#[derive(Deserialize, Serialize)]
struct Mutant {
    name: String,
    file: String,
    package: String,
}

#[derive(Deserialize)]
enum Scenario {
    Baseline,
    Mutant(Mutant),
}

#[derive(Deserialize, PartialEq, Eq)]
enum Summary {
    Success,
    Failure,
    CaughtMutant,
    MissedMutant,
    Timeout,
    Unviable,
}

#[derive(Deserialize)]
struct Phase {
    phase: String,
    process_status: serde_json::Value,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct CompilerDiagnostic {
    level: String,
    code: Option<CompilerCode>,
    message: String,
    #[serde(default)]
    children: Vec<CompilerDiagnostic>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct CompilerCode {
    code: String,
}

#[derive(Deserialize)]
struct Outcome {
    scenario: Scenario,
    summary: Summary,
    phase_results: Vec<Phase>,
}

#[derive(Deserialize)]
struct Report {
    cargo_mutants_version: String,
    total_mutants: usize,
    caught: usize,
    missed: usize,
    timeout: usize,
    unviable: usize,
    success: usize,
    end_time: Option<String>,
    outcomes: Vec<Outcome>,
}

fn validate(report: &Report, expected: &BTreeSet<String>) -> anyhow::Result<()> {
    if expected.is_empty()
        || report.cargo_mutants_version != MUTANTS_VERSION
        || report.end_time.as_ref().is_none_or(String::is_empty)
    {
        bail!("Empty inventory, wrong tool version, or incomplete mutation report");
    }
    let mut actual = BTreeSet::new();
    let mut baseline = 0;
    let mut caught = 0;
    let mut unviable = 0;
    for outcome in &report.outcomes {
        let phases = &outcome.phase_results;
        let built = phases
            .first()
            .is_some_and(|phase| phase.phase == "Build" && phase.process_status == "Success");
        let tested = phases.len() == BUILD_AND_TEST_PHASES && phases[1].phase == "Test";
        match &outcome.scenario {
            Scenario::Baseline => {
                baseline += 1;
                if outcome.summary != Summary::Success
                    || !built
                    || !tested
                    || phases[1].process_status != "Success"
                {
                    bail!("The unmutated baseline did not build and pass its tests");
                }
            }
            Scenario::Mutant(mutant) => {
                if !actual.insert(mutant.name.clone()) {
                    bail!("A mutant was counted twice: {}", mutant.name);
                }
                match outcome.summary {
                    Summary::CaughtMutant
                        if built
                            && tested
                            && phases[1].process_status
                                == serde_json::json!({"Failure":CARGO_FAILURE_EXIT}) =>
                    {
                        caught += 1;
                    }
                    Summary::Unviable
                        if phases.len() == 1
                            && phases[0].phase == "Build"
                            && phases[0].process_status
                                == serde_json::json!({"Failure":CARGO_FAILURE_EXIT}) =>
                    {
                        unviable += 1;
                    }
                    _ => bail!(
                        "Missed, inconclusive, or inconsistent mutant: {}",
                        mutant.name
                    ),
                }
            }
        }
    }
    if baseline != 1
        || &actual != expected
        || report.total_mutants != expected.len()
        || report.caught != caught
        || report.unviable != unviable
        || report.missed != 0
        || report.timeout != 0
        || report.success != 0
        || caught + unviable != expected.len()
    {
        bail!("The mutation report does not establish its complete inventory");
    }
    Ok(())
}

fn selected(
    command: &mut Command,
    scope: Scope,
    file: Option<&str>,
    matching: Option<&str>,
    shard: Option<&str>,
) {
    match scope {
        Scope::Domain => {
            command.args(["--package", "law-domain", "--test-workspace", "true"]);
        }
        Scope::Runtime => {
            command.args([
                "--package",
                "law-domain",
                "--package",
                "law-history",
                "--package",
                "law-history-cli",
                "--test-workspace",
                "true",
            ]);
        }
        Scope::Tooling => {
            command.args(["--package", "xtask", "--test-package", "xtask"]);
        }
    }
    if let Some(file) = file {
        command.args(["--file", file]);
    }
    if let Some(matching) = matching {
        command.args(["--re", matching]);
    }
    if let Some(shard) = shard {
        command.args(["--shard", shard, "--sharding", "round-robin"]);
    }
}

fn inventory(
    root: &Path,
    image: &str,
    scope: Scope,
    file: Option<&str>,
    matching: Option<&str>,
    shard: Option<&str>,
    name: &str,
) -> anyhow::Result<Vec<Mutant>> {
    let mut command = process(root, image, name);
    selected(&mut command, scope, file, matching, shard);
    command.args(["--list", "--json"]);
    let output = bounded_output(&mut command, name)?;
    require_success(&output)?;
    serde_json::from_str(&output.stdout).context("The native mutation inventory is not valid JSON")
}

fn validate_native_logs(directory: &Path, report: &Report) -> anyhow::Result<()> {
    let native: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("outcomes.json"))?)?;
    for (outcome, value) in report.outcomes.iter().zip(
        native["outcomes"]
            .as_array()
            .context("Missing native outcomes")?,
    ) {
        let path = value["log_path"]
            .as_str()
            .context("Missing native log path")?;
        if !Path::new(path)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            bail!("The native report contains an unsafe log path");
        }
        let mut bytes = Vec::new();
        fs::File::open(directory.join(path))?
            .take(u64::try_from(native_log::MAXIMUM_BYTES)? + 1)
            .read_to_end(&mut bytes)?;
        if !native_log::allows(bytes.len()) {
            bail!("Native mutation log exceeds the 8 MiB diagnostic limit: {path}");
        }
        let log = String::from_utf8(bytes).context("Native mutation log isn't valid UTF-8")?;
        for line in log.lines().map(str::trim_start).filter(|line| {
            line.starts_with("error:")
                || line.starts_with("error[")
                || line.starts_with("process didn't exit successfully:")
                || line.starts_with("INCONCLUSIVE:")
                || line.starts_with("LLVM ERROR:")
                || line.starts_with("fatal runtime error:")
        }) {
            for marker in INFRASTRUCTURE_MARKERS {
                if line.contains(marker) {
                    bail!("Infrastructure failure in {path}: {marker}");
                }
            }
        }
        if outcome.summary == Summary::CaughtMutant && !log.contains("test result: FAILED.") {
            bail!("A caught mutant lacks a native test failure: {path}");
        }
        let mut compiler_error = false;
        for line in log.lines().filter(|line| line.starts_with('{')) {
            if let Ok(message) = serde_json::from_str::<serde_json::Value>(line)
                && message["reason"] == "compiler-message"
            {
                let diagnostic: CompilerDiagnostic =
                    serde_json::from_value(message["message"].clone())
                        .context("Malformed native compiler diagnostic")?;
                if diagnostic.level != "error" {
                    continue;
                }
                let mut pending = vec![&diagnostic];
                while let Some(diagnostic) = pending.pop() {
                    if diagnostic
                        .code
                        .as_ref()
                        .is_some_and(|code| code.code == "E0786")
                    {
                        bail!(
                            "Invalid compiler metadata leaves mutation evidence incomplete: {path}"
                        );
                    }
                    for marker in INFRASTRUCTURE_MARKERS {
                        if marker != "SIGKILL" && diagnostic.message.contains(marker) {
                            bail!("Infrastructure compiler diagnostic in {path}: {marker}");
                        }
                    }
                    pending.extend(&diagnostic.children);
                }
                compiler_error = true;
            }
        }
        if outcome.summary == Summary::Unviable && !compiler_error {
            bail!("An unviable mutant lacks a Rust compiler diagnostic: {path}");
        }
    }
    Ok(())
}

fn process(root: &Path, image: &str, name: &str) -> Command {
    let mut command = Command::new("docker");
    command.args([
        "run",
        "--init",
        "--name",
        name,
        "--network",
        "none",
        "--memory",
        "2048m",
        "--memory-swap",
        "2048m",
        "--cpus",
        "2",
        "--pids-limit",
        "256",
        "--tmpfs",
        &format!("/scratch:rw,nosuid,nodev,size={SCRATCH_MEMORY_MIB}m"),
        "--env",
        "TMPDIR=/scratch",
        "--mount",
        &format!("type=bind,source={},target=/work,readonly", root.display()),
        "--workdir",
        "/work",
        "--env",
        "CARGO_BUILD_JOBS=2",
        "--env",
        "CARGO_INCREMENTAL=0",
        "--env",
        "CARGO_PROFILE_DEV_DEBUG=0",
        "--env",
        "CARGO_PROFILE_TEST_DEBUG=0",
        "--mount",
        "type=volume,target=/tmp/law-history-verification,volume-driver=local,volume-nocopy",
        "--env",
        "CARGO_TARGET_DIR=/tmp/law-history-verification",
        image,
        "cargo",
        "mutants",
        "--no-shuffle",
        "--jobs",
        "1",
        "--no-times",
        "--timeout",
        MUTANT_TEST_TIMEOUT_SECONDS,
        "--build-timeout",
        MUTANT_BUILD_TIMEOUT_SECONDS,
    ]);
    command
}

fn require_success(result: &Captured) -> anyhow::Result<()> {
    if result.timed_out || result.memory_exceeded || result.code != Some(0) {
        bail!(
            "Mutation verification did not pass (exit {:?}, resource stop {}); inspect the retained native report",
            result.code,
            result.timed_out || result.memory_exceeded
        );
    }
    Ok(())
}

pub fn campaign(
    root: &Path,
    supplied: Option<String>,
    scope: Scope,
    file: Option<String>,
    matching: Option<String>,
    shard: Option<String>,
    list: bool,
) -> anyhow::Result<()> {
    let image = image(root, supplied)?;
    let sources = source_inventory(root, true, "crates/law-domain/src/lib.rs")?;
    let source_hash = law_history::store::sha256(&serde_json::to_vec(&sources)?);
    let snapshot = snapshot_sources(root, &sources)?;
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = format!("law-history-mutations-{}-{sequence}", std::process::id());
    let inventory = inventory(
        snapshot.path(),
        &image,
        scope,
        file.as_deref(),
        matching.as_deref(),
        shard.as_deref(),
        &name,
    )?;
    let expected: BTreeSet<_> = inventory.iter().map(|mutant| mutant.name.clone()).collect();
    if expected.is_empty() || expected.len() != inventory.len() {
        bail!("The selected mutation inventory is empty or contains duplicate identities");
    }
    let selection = serde_json::json!({"scope":format!("{scope:?}"),"file":file,"matching":matching,"shard":shard});
    eprintln!(
        "Mutation inventory: {} candidates; {selection}",
        expected.len()
    );
    let result_root = root.join("verification/results");
    fs::create_dir_all(&result_root)?;
    let run_name = format!("mutation-{}-{sequence}", std::process::id());
    let directory = result_root.join(&run_name);
    fs::create_dir(&directory)?;
    fs::write(
        directory.join("inventory.json"),
        serde_json::to_vec_pretty(&inventory)?,
    )?;
    let receipt_path = directory.join("receipt.json");
    let mut receipt = serde_json::json!({
        "schema_version":1,"image":image,"source_sha256":source_hash,"source_files":sources,
        "selection":selection,"expected_mutants":expected.len(),"complete":false,
        "inventory_only":list,"native_report":format!("{run_name}/mutants.out/outcomes.json"),
        "limits":{"memory_mib":CONTAINER_MEMORY_MIB,"cpus":CONTAINER_CPUS,"pids":CONTAINER_PIDS,"container_seconds":CONTAINER_TIMEOUT_SECONDS,"test_seconds":TEST_TIMEOUT_SECONDS,"build_seconds":BUILD_TIMEOUT_SECONDS,"scratch_memory_mib":SCRATCH_MEMORY_MIB},
        "tool_version":MUTANTS_VERSION,
        "build":{"incremental":false,"dev_debug":0,"test_debug":0,"cache":"container-private","storage":"anonymous-local-volume"},
    });
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    if list {
        return Ok(());
    }
    let mut run_command = process(snapshot.path(), &image, &name);
    let image_position = run_command
        .get_args()
        .position(|argument| argument == image.as_str())
        .context("Missing container image")?;
    let arguments: Vec<_> = run_command
        .get_args()
        .map(|argument| argument.to_os_string())
        .collect();
    run_command = Command::new("docker");
    run_command.args(&arguments[..image_position]);
    run_command.args([
        "--mount",
        &format!("type=bind,source={},target=/results", result_root.display()),
    ]);
    run_command.args(&arguments[image_position..]);
    selected(
        &mut run_command,
        scope,
        file.as_deref(),
        matching.as_deref(),
        shard.as_deref(),
    );
    run_command.args(["--output", &format!("/results/{run_name}")]);
    let result = bounded_output(&mut run_command, &name)?;
    fs::write(directory.join("stdout.log"), &result.stdout)?;
    fs::write(directory.join("stderr.log"), &result.stderr)?;
    for line in result
        .stdout
        .lines()
        .chain(result.stderr.lines())
        .take(LOG_PREVIEW_LINES)
    {
        eprintln!("{line}");
    }
    eprintln!("Native logs: {}", directory.display());
    receipt["exit"] = serde_json::json!(result.code);
    receipt["resource_stop"] = serde_json::json!(result.timed_out || result.memory_exceeded);
    receipt["memory_exceeded"] = serde_json::json!(result.memory_exceeded);
    let report_path = directory.join("mutants.out/outcomes.json");
    if let Ok(bytes) = fs::read(&report_path) {
        receipt["native_report_sha256"] = serde_json::json!(law_history::store::sha256(&bytes));
    }
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    require_success(&result)?;
    let report: Report = serde_json::from_slice(&fs::read(report_path)?)?;
    validate(&report, &expected)?;
    validate_native_logs(&directory.join("mutants.out"), &report)?;
    if sources != source_inventory(root, true, "crates/law-domain/src/lib.rs")? {
        bail!("Source or tests changed during mutation verification");
    }
    receipt["complete"] = serde_json::json!(true);
    receipt["caught"] = serde_json::json!(report.caught);
    receipt["unviable"] = serde_json::json!(report.unviable);
    fs::write(receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    eprintln!(
        "Mutation verification passed: {} caught, {} unviable, 0 missed, 0 timeouts",
        report.caught, report.unviable
    );
    Ok(())
}

pub fn suite(root: &Path, options: Options) -> anyhow::Result<()> {
    let Options {
        image: supplied,
        scope,
        file,
        matching,
        shard,
        list,
        shards,
        jobs,
    } = options;
    if shards == 0
        || jobs == 0
        || jobs > MAXIMUM_MUTATION_WORKERS
        || (shard.is_some() && shards != 1)
    {
        bail!(
            "Use a nonempty shard inventory, one through six workers, and either one shard selection or a complete suite"
        );
    }
    if list || shards == 1 {
        return campaign(root, supplied, scope, file, matching, shard, list);
    }
    let image = image(root, supplied)?;
    let sources = source_inventory(root, true, "crates/law-domain/src/lib.rs")?;
    fs::create_dir_all(root.join("verification/results"))?;
    fs::write(
        root.join("verification/results/mutation-suite.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version":1,"image":image,"source_files":sources,
            "source_sha256":law_history::store::sha256(&serde_json::to_vec(&sources)?),
            "scope":format!("{scope:?}"),"file":file,"matching":matching,"shards":shards,"jobs":jobs,"complete":false
        }))?,
    )?;
    let expected: BTreeSet<_> = inventory(
        root,
        &image,
        scope,
        file.as_deref(),
        matching.as_deref(),
        None,
        &format!("law-history-inventory-{}", std::process::id()),
    )?
    .into_iter()
    .map(|mutant| mutant.name)
    .collect();
    if expected.is_empty() || shards > expected.len() {
        bail!("The complete suite needs nonempty shards");
    }
    let first_sequence = RUN_SEQUENCE.load(Ordering::Relaxed);
    let next = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    std::thread::scope(|threads| {
        for _ in 0..jobs {
            let image = &image;
            let next = &next;
            let failed = &failed;
            let file = &file;
            let matching = &matching;
            threads.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= shards {
                        break;
                    }
                    if let Err(error) = campaign(
                        root,
                        Some(image.clone()),
                        scope,
                        file.clone(),
                        matching.clone(),
                        Some(format!("{index}/{shards}")),
                        false,
                    ) {
                        eprintln!("Mutation shard {index}/{shards} did not pass: {error:#}");
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    let mut actual = BTreeSet::new();
    let mut duplicate = false;
    let mut caught = 0;
    let mut missed = 0;
    let mut unviable = 0;
    let mut timeout = 0;
    let mut incomplete = Vec::new();
    for sequence in first_sequence..RUN_SEQUENCE.load(Ordering::Relaxed) {
        let directory = root.join(format!(
            "verification/results/mutation-{}-{sequence}",
            std::process::id()
        ));
        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("receipt.json"))?)?;
        if receipt["complete"] != true {
            incomplete.push(directory.to_string_lossy().into_owned());
        }
        if receipt["source_sha256"] != law_history::store::sha256(&serde_json::to_vec(&sources)?) {
            incomplete.push(format!("Source identity differs: {}", directory.display()));
        }
        let path = directory.join("mutants.out/outcomes.json");
        if path.exists() {
            let report: Report = serde_json::from_slice(&fs::read(path)?)?;
            caught += report.caught;
            missed += report.missed;
            unviable += report.unviable;
            timeout += report.timeout;
            for outcome in report.outcomes {
                if let Scenario::Mutant(mutant) = outcome.scenario {
                    duplicate |= !actual.insert(mutant.name);
                }
            }
        }
    }
    let unchanged = sources == source_inventory(root, true, "crates/law-domain/src/lib.rs")?;
    let complete = failed.load(Ordering::Relaxed) == 0
        && incomplete.is_empty()
        && !duplicate
        && actual == expected
        && unchanged
        && caught > 0
        && missed == 0
        && timeout == 0;
    let record = serde_json::json!({
        "schema_version":1,"image":image,"source_sha256":law_history::store::sha256(&serde_json::to_vec(&sources)?),"source_files":sources,
        "scope":format!("{scope:?}"),"file":file,"matching":matching,"shards":shards,"jobs":jobs,"complete":complete,
        "expected":expected.len(),"executed":actual.len(),"caught":caught,"missed":missed,"unviable":unviable,"timeout":timeout,
        "source_unchanged":unchanged,"duplicate":duplicate,"incomplete":incomplete,
        "missing":expected.difference(&actual).collect::<Vec<_>>(),"unexpected":actual.difference(&expected).collect::<Vec<_>>(),
    });
    fs::write(
        root.join("verification/results/mutation-suite.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;
    eprintln!(
        "Mutation suite: {caught} caught, {missed} missed, {unviable} unviable, {timeout} timeouts; complete={complete}"
    );
    if !complete {
        bail!("The complete mutation suite did not pass; inspect the retained reports");
    }
    eprintln!("All {shards} mutation shards passed");
    emit_completed_evidence(
        &law_history::store::sha256(&serde_json::to_vec(&sources)?),
        &law_history::store::sha256(&fs::read(root.join(".cargo/mutants.toml"))?),
        EvidenceMethod::Mutation,
        MUTANTS_VERSION,
        &image,
        &format!(
            "{shards} shards, {jobs} workers; {scope:?}; file={file:?}; matching={matching:?}; every selected native scenario; {CONTAINER_CPUS} CPUs, {CONTAINER_MEMORY_MIB} MiB, {CONTAINER_PIDS} PIDs, {CONTAINER_TIMEOUT_SECONDS} seconds per container; build={BUILD_TIMEOUT_SECONDS}s, test={TEST_TIMEOUT_SECONDS}s; scratch={SCRATCH_MEMORY_MIB} MiB"
        ),
        &[("complete-native-mutation-inventory", actual.len())],
    )?;
    Ok(())
}

pub fn release(root: &Path) -> anyhow::Result<()> {
    suite(
        root,
        Options {
            image: None,
            scope: Scope::Tooling,
            file: Some("crates/xtask/src/mutation.rs".into()),
            matching: Some("validate_native_logs|selected".into()),
            shard: None,
            list: false,
            shards: NATIVE_DIAGNOSTIC_MUTATION_SHARDS,
            jobs: 1,
        },
    )?;
    suite(
        root,
        Options {
            image: None,
            scope: Scope::Tooling,
            file: Some("crates/xtask/src/image_reference.rs".into()),
            matching: None,
            shard: None,
            list: false,
            shards: IMAGE_REFERENCE_MUTATION_SHARDS,
            jobs: 1,
        },
    )?;
    suite(
        root,
        Options {
            image: None,
            scope: Scope::Tooling,
            file: Some("crates/xtask/src/native_log.rs".into()),
            matching: None,
            shard: None,
            list: false,
            shards: NATIVE_LOG_MUTATION_SHARDS,
            jobs: 1,
        },
    )?;
    suite(
        root,
        Options {
            image: None,
            scope: Scope::Runtime,
            file: None,
            matching: None,
            shard: None,
            list: false,
            shards: RELEASE_MUTATION_SHARDS,
            jobs: 1,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_build_outputs_use_fresh_linux_local_storage_without_host_cache_mounts() {
        let command = process(Path::new("/source"), "image", "owned");
        let arguments: Vec<_> = command.get_args().collect();
        assert!(arguments.windows(2).any(|pair| {
            pair == ["--mount", "type=volume,target=/tmp/law-history-verification,volume-driver=local,volume-nocopy"]
        }));
        assert!(
            arguments.windows(2).any(|pair| {
                pair == ["--env", "CARGO_TARGET_DIR=/tmp/law-history-verification"]
            })
        );
    }

    #[test]
    fn native_selection_checks_workspace_consumers_for_shared_source_mutations() {
        for (scope, expected) in [
            (
                Scope::Domain,
                vec!["--package", "law-domain", "--test-workspace", "true"],
            ),
            (
                Scope::Runtime,
                vec![
                    "--package",
                    "law-domain",
                    "--package",
                    "law-history",
                    "--package",
                    "law-history-cli",
                    "--test-workspace",
                    "true",
                ],
            ),
            (
                Scope::Tooling,
                vec!["--package", "xtask", "--test-package", "xtask"],
            ),
        ] {
            for file in [None, Some("crates/xtask/src/native_log.rs")] {
                for matching in [None, Some("validate_native_logs|selected")] {
                    for shard in [None, Some("1/3")] {
                        let mut command = Command::new("cargo");
                        selected(&mut command, scope, file, matching, shard);
                        let mut arguments = expected.clone();
                        if let Some(file) = file {
                            arguments.extend(["--file", file]);
                        }
                        if let Some(matching) = matching {
                            arguments.extend(["--re", matching]);
                        }
                        if let Some(shard) = shard {
                            arguments.extend(["--shard", shard, "--sharding", "round-robin"]);
                        }
                        assert_eq!(command.get_args().collect::<Vec<_>>(), arguments);
                    }
                }
            }
        }
    }

    fn report() -> serde_json::Value {
        serde_json::json!({
            "cargo_mutants_version":MUTANTS_VERSION,"total_mutants":1,"caught":1,"missed":0,"timeout":0,"unviable":0,"success":0,
            "end_time":"2026-11-05T00:00:00Z",
            "outcomes":[
                {"scenario":"Baseline","summary":"Success","phase_results":[{"phase":"Build","process_status":"Success"},{"phase":"Test","process_status":"Success"}]},
                {"scenario":{"Mutant":{"name":"contract","file":"source.rs","package":"domain"}},"summary":"CaughtMutant","phase_results":[{"phase":"Build","process_status":"Success"},{"phase":"Test","process_status":{"Failure":101}}]}
            ]
        })
    }

    fn check(value: serde_json::Value) -> anyhow::Result<()> {
        validate(
            &serde_json::from_value(value)?,
            &BTreeSet::from(["contract".into()]),
        )
    }

    #[test]
    fn gate_requires_complete_current_inventory_and_real_test_failures() {
        assert!(check(report()).is_ok());
        for (field, value) in [
            ("end_time", serde_json::Value::Null),
            ("cargo_mutants_version", serde_json::json!("different")),
            ("total_mutants", serde_json::json!(0)),
            ("caught", serde_json::json!(0)),
            ("missed", serde_json::json!(1)),
            ("timeout", serde_json::json!(1)),
        ] {
            let mut broken = report();
            broken[field] = value;
            assert!(check(broken).is_err(), "{field}");
        }
        for summary in ["MissedMutant", "Timeout", "Failure", "Success"] {
            let mut broken = report();
            broken["outcomes"][1]["summary"] = serde_json::json!(summary);
            assert!(check(broken).is_err());
        }
        let mut wrong_identity = report();
        wrong_identity["outcomes"][1]["scenario"]["Mutant"]["name"] = serde_json::json!("stale");
        assert!(check(wrong_identity).is_err());
        let mut wrong_failure = report();
        wrong_failure["outcomes"][1]["phase_results"][1]["process_status"] =
            serde_json::json!({"Failure":137});
        assert!(check(wrong_failure).is_err());
        let mut missing_baseline = report();
        missing_baseline["outcomes"]
            .as_array_mut()
            .unwrap()
            .remove(0);
        assert!(check(missing_baseline).is_err());
        assert!(validate(&serde_json::from_value(report()).unwrap(), &BTreeSet::new()).is_err());
    }

    #[test]
    fn native_log_budget_includes_its_boundary_and_refuses_larger_files() {
        const BOUNDARY: usize = 8_388_608;
        let directory = tempfile::tempdir().unwrap();
        let mut native = report();
        for outcome in native["outcomes"].as_array_mut().unwrap() {
            outcome["log_path"] = serde_json::json!("case.log");
        }
        fs::write(
            directory.path().join("outcomes.json"),
            serde_json::to_vec(&native).unwrap(),
        )
        .unwrap();
        let report = serde_json::from_value(native).unwrap();
        let marker = b"test result: FAILED.\n";
        for length in [BOUNDARY - 1, BOUNDARY, BOUNDARY + 1] {
            let mut bytes = vec![b' '; length];
            bytes[..marker.len()].copy_from_slice(marker);
            fs::write(directory.path().join("case.log"), bytes).unwrap();
            let result = validate_native_logs(directory.path(), &report);
            assert_eq!(result.is_ok(), length <= BOUNDARY, "{length}: {result:?}");
        }
    }

    #[test]
    fn native_logs_require_real_test_or_compiler_failures_and_refuse_resource_stops() {
        let sandbox = tempfile::tempdir().unwrap();
        let directory = sandbox.path().join("logs");
        fs::create_dir(&directory).unwrap();
        let mut native = report();
        for outcome in native["outcomes"].as_array_mut().unwrap() {
            outcome["log_path"] = serde_json::json!("case.log");
        }
        let write = |value: &serde_json::Value, log: &str| {
            fs::write(
                directory.join("outcomes.json"),
                serde_json::to_vec(value).unwrap(),
            )
            .unwrap();
            fs::write(directory.join("case.log"), log).unwrap();
            validate_native_logs(&directory, &serde_json::from_value(value.clone()).unwrap())
        };
        assert!(write(&native, "test result: FAILED.").is_ok());
        assert!(
            write(
                &native,
                "assertion failed: expected a refusal for SIGKILL\ntest result: FAILED."
            )
            .is_ok()
        );
        assert!(write(&native, "error: arbitrary tool failure").is_err());
        for marker in ["SIGKILL", "No space left on device", "INCONCLUSIVE:"] {
            assert!(write(&native, &format!("test result: FAILED.\nerror: {marker}")).is_err());
        }
        for diagnostic in [
            "  process didn't exit successfully: `rustc` (signal: 9, SIGKILL: kill)",
            "INCONCLUSIVE: stopped",
            "LLVM ERROR: out of memory",
            "fatal runtime error: out of memory",
            "error: internal compiler error",
            "error: the compiler unexpectedly panicked",
            "error[E0786]: Cannot allocate memory",
            "error: failed to download `dependency`",
            "error: failed to get `dependency`",
        ] {
            assert!(write(&native, &format!("test result: FAILED.\n{diagnostic}")).is_err());
        }
        native["outcomes"][1]["summary"] = serde_json::json!("Unviable");
        let diagnostic = "{\"reason\":\"compiler-message\",\"message\":{\"level\":\"error\",\"code\":null,\"message\":\"syntax error\"}}";
        assert!(write(&native, diagnostic).is_ok());
        let exhausted_storage = serde_json::json!({
            "reason":"compiler-message",
            "message":{
                "level":"error", "code":null,
                "message":"couldn't write metadata: No space left on device",
                "children":[]
            }
        })
        .to_string();
        assert!(write(&native, &exhausted_storage).is_err());
        let mut nested_storage: serde_json::Value = serde_json::from_str(diagnostic).unwrap();
        nested_storage["message"]["children"] = serde_json::json!([{
            "level":"note", "code":null, "message":"dependency metadata",
            "children":[{"level":"note","code":null,"message":"Cannot allocate memory","children":[]}]
        }]);
        assert!(write(&native, &nested_storage.to_string()).is_err());
        let base: serde_json::Value = serde_json::from_str(diagnostic).unwrap();
        for message in [
            "process failed (signal: 9)",
            "out of memory",
            "No space left on device",
            "Cannot allocate memory",
            "failed to download dependency",
            "failed to get `dependency`",
            "INCONCLUSIVE: stopped",
            "internal compiler error",
            "compiler unexpectedly panicked",
        ] {
            for depth in 0..=2 {
                let mut node = serde_json::json!({
                    "level":"note", "code":null, "message":message, "children":[]
                });
                for _ in 0..depth {
                    node = serde_json::json!({
                        "level":"note", "code":null, "message":"dependency context", "children":[node]
                    });
                }
                let mut nested = base.clone();
                nested["message"]["children"] = serde_json::json!([node]);
                assert!(
                    write(&native, &nested.to_string()).is_err(),
                    "{message}, {depth}"
                );
                let mut primary = base.clone();
                primary["message"]["message"] = serde_json::json!(message);
                assert!(write(&native, &primary.to_string()).is_err(), "{message}");
            }
        }
        let parsed: CompilerDiagnostic = serde_json::from_value(base["message"].clone()).unwrap();
        let fields: BTreeSet<_> = serde_json::to_value(parsed)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let probes = [
            ("level", serde_json::json!("warning")),
            ("code", serde_json::json!({"code":"E0786"})),
            ("message", serde_json::json!("No space left on device")),
            (
                "children",
                serde_json::json!([{
                    "level":"note", "code":null, "message":"out of memory", "children":[]
                }]),
            ),
        ];
        assert_eq!(
            fields,
            probes
                .iter()
                .map(|(field, _)| (*field).to_owned())
                .collect()
        );
        let code_fields = serde_json::to_value(CompilerCode {
            code: "E0308".into(),
        })
        .unwrap();
        assert_eq!(
            code_fields
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["code"]
        );
        for (field, value) in probes {
            let mut changed = base.clone();
            changed["message"][field] = value;
            assert!(write(&native, &changed.to_string()).is_err(), "{field}");
        }
        for field in ["level", "message"] {
            let mut malformed = base.clone();
            malformed["message"].as_object_mut().unwrap().remove(field);
            assert!(write(&native, &malformed.to_string()).is_err(), "{field}");
        }
        let quoted_source = serde_json::json!({
            "reason":"compiler-message",
            "message":{
                "level":"error", "code":{"code":"E0308","explanation":"out of memory; SIGKILL"}, "message":"mismatched types",
                "spans":[{"text":[{"text":"let fixture = \"failed to download; signal: 9; SIGKILL\";"}]}]
            }
        }).to_string();
        assert!(write(&native, &quoted_source).is_ok());
        let symbol_name = quoted_source.replace(
            "mismatched types",
            "cannot find value `SIGKILL` in this scope",
        );
        assert!(write(&native, &symbol_name).is_ok());
        let invalid_metadata = serde_json::json!({
            "reason":"compiler-message",
            "message":{"level":"error","code":{"code":"E0786"},"message":"found invalid metadata files for crate `tokio`"}
        }).to_string();
        assert!(write(&native, &invalid_metadata).is_err());
        let semantic_error = invalid_metadata.replace("E0786", "E0308");
        assert!(write(&native, &semantic_error).is_ok());
        for reason in ["compiler-message", "other", ""] {
            for level in ["error", "warning", "note"] {
                for code in [None, Some("E0308"), Some("E0786")] {
                    let diagnostic = serde_json::json!({
                        "reason":reason,
                        "message":{"level":level,"code":code.map(|code| serde_json::json!({"code":code})),"message":"syntax error"}
                    }).to_string();
                    let expected = matches!(
                        (reason, level, code),
                        ("compiler-message", "error", None | Some("E0308"))
                    );
                    assert_eq!(
                        write(&native, &diagnostic).is_ok(),
                        expected,
                        "{diagnostic}"
                    );
                }
            }
        }
        for log in [
            format!("{semantic_error}\n{invalid_metadata}"),
            format!("{invalid_metadata}\n{semantic_error}"),
        ] {
            assert!(write(&native, &log).is_err());
        }
        assert!(write(&native, &diagnostic.replace("\"error\"", "\"warning\"")).is_err());
        assert!(
            write(
                &native,
                "error[E0308]: plain output without a Cargo message"
            )
            .is_err()
        );
        native["outcomes"][1]["log_path"] = serde_json::json!("../case.log");
        fs::write(sandbox.path().join("case.log"), diagnostic).unwrap();
        assert_eq!(
            write(&native, diagnostic).unwrap_err().to_string(),
            "The native report contains an unsafe log path"
        );
        for (timed_out, memory_exceeded, code) in [
            (true, false, Some(0)),
            (false, true, Some(0)),
            (false, false, Some(137)),
            (false, false, None),
        ] {
            assert!(
                require_success(&Captured {
                    code,
                    stdout: String::new(),
                    stderr: String::new(),
                    timed_out,
                    memory_exceeded
                })
                .is_err()
            );
        }
        assert!(
            require_success(&Captured {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false,
                memory_exceeded: false
            })
            .is_ok()
        );
    }
}

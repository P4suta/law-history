use crate::{model::validate_id, store::sha256};
use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn checked_path(path: &str) -> bool {
    path == "selection.json"
        || path.split_once('/').is_some_and(|(law, file)| {
            validate_id(law).is_ok()
                && matches!(file, "text.txt" | "selected.json" | "history.json")
        })
}

pub fn import_snapshot(
    directory: &Path,
    files: &BTreeMap<String, String>,
    generated_at: DateTime<Utc>,
) -> anyhow::Result<()> {
    if files.is_empty() || files.keys().any(|path| !checked_path(path)) {
        bail!("A Git data export accepts only generated law snapshot files");
    }
    if fs::symlink_metadata(directory.join(".git")).is_ok() {
        bail!("Git data export requires a destination without repository metadata");
    }
    let initialized = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["init", "--initial-branch=main"])
        .output()?;
    if !initialized.status.success() {
        bail!(
            "Cannot initialize the data export: {}",
            String::from_utf8_lossy(&initialized.stderr)
        );
    }
    let mut importer = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let write_result = (|| {
        let mut stream = importer
            .stdin
            .take()
            .context("Git import input is unavailable")?;
        writeln!(
            stream,
            "feature done\ncommit refs/heads/main\ncommitter law-history export <law-history@localhost> {} +0000",
            generated_at.timestamp()
        )?;
        let message = b"feat(data): export legislation snapshot\n";
        writeln!(stream, "data {}", message.len())?;
        stream.write_all(message)?;
        writeln!(stream)?;
        for (path, expected) in files {
            let bytes = fs::read(directory.join(path))?;
            if sha256(&bytes) != *expected {
                bail!("Generated export changed before import: {path}");
            }
            writeln!(stream, "M 100644 inline {path}\ndata {}", bytes.len())?;
            stream.write_all(&bytes)?;
            writeln!(stream)?;
        }
        writeln!(stream, "\ndone")?;
        Ok::<_, anyhow::Error>(())
    })();
    let imported = importer.wait_with_output()?;
    write_result?;
    if !imported.status.success() {
        bail!(
            "Git data import failed: {}",
            String::from_utf8_lossy(&imported.stderr)
        );
    }
    let indexed = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["read-tree", "HEAD"])
        .output()?;
    if !indexed.status.success() {
        bail!(
            "The imported data tree cannot be indexed: {}",
            String::from_utf8_lossy(&indexed.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::checked_path;

    #[test]
    fn import_refuses_empty_and_invalid_manifests_before_initializing_git() {
        for files in [
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::from([("src/main.rs".into(), "untrusted".into())]),
        ] {
            let directory = tempfile::tempdir().unwrap();
            assert!(super::import_snapshot(directory.path(), &files, chrono::Utc::now()).is_err());
            assert!(!directory.path().join(".git").exists());
        }
    }
    #[test]
    fn data_exports_refuse_source_and_control_paths() {
        for path in [
            "selection.json",
            "321CONSTITUTION/text.txt",
            "DEMO_ACT/history.json",
        ] {
            assert!(checked_path(path));
        }
        for path in [
            ".git/config",
            "../selection.json",
            "DEMO_ACT/../../src/main.rs",
            "DEMO_ACT/main.rs",
            "src/lib.rs",
        ] {
            assert!(!checked_path(path));
        }
    }
}

use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand};
use law_history::{
    Store,
    amendment::{self, Scenario},
    api, git_export,
    ingest::{self, Fetcher},
    model::*,
    parliament, reconstruction, service,
};
use serde::Serialize;
use std::{fs, net::SocketAddr, path::PathBuf, time::Duration};

const EGOV_MINIMUM_DELAY: Duration = Duration::from_millis(250);
const SPEECH_MINIMUM_DELAY: Duration = Duration::from_secs(3);

#[derive(Parser)]
#[command(
    name = "law-history",
    about = "Preserve and inspect Japanese legislation across enforcement and observation time"
)]
struct Cli {
    #[arg(long, global = true, default_value = ".law-history")]
    store: PathBuf,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Clone, Default)]
struct Temporal {
    #[arg(long)]
    at: Option<Day>,
    #[arg(long)]
    known_at: Option<DateTime<Utc>>,
    #[arg(long)]
    version: Option<String>,
}
impl From<Temporal> for Query {
    fn from(value: Temporal) -> Self {
        Self {
            at: value.at,
            known_at: value.known_at,
            version: value.version,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    Sync {
        #[arg(long, conflicts_with_all = ["law", "import", "bill"])]
        all: bool,
        #[arg(long, conflicts_with_all = ["import", "bill"])]
        law: Option<String>,
        #[arg(long)]
        catalog_only: bool,
        #[arg(long)]
        import: Option<PathBuf>,
        #[arg(long)]
        bill: Option<PathBuf>,
        #[arg(long)]
        refresh: bool,
        #[arg(long)]
        max_laws: Option<usize>,
        #[arg(long)]
        max_requests: Option<usize>,
    },
    Search {
        text: String,
        #[command(flatten)]
        time: Temporal,
    },
    Log {
        law: String,
        #[command(flatten)]
        time: Temporal,
    },
    Show {
        law: String,
        #[command(flatten)]
        time: Temporal,
    },
    Diff {
        from: String,
        to: String,
    },
    Blame {
        law: String,
        path: String,
        #[command(flatten)]
        time: Temporal,
    },
    Refs {
        law: String,
        #[command(flatten)]
        time: Temporal,
    },
    Impact {
        from: String,
        to: String,
        #[command(flatten)]
        time: Temporal,
    },
    Simulate {
        file: PathBuf,
        #[arg(long)]
        save: bool,
    },
    Scenario {
        id: String,
    },
    Export {
        #[command(subcommand)]
        format: Export,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1:3000")]
        bind: SocketAddr,
        #[arg(long, default_value = "web/dist")]
        web: PathBuf,
    },
    Status,
    Reindex,
    Verify,
    ImportCache {
        #[arg(long)]
        from: PathBuf,
    },
    Schema {
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Parliament {
        law: String,
        #[command(flatten)]
        time: Temporal,
    },
    Speeches {
        #[arg(long)]
        session: u32,
        query: String,
        #[arg(long, default_value_t = 1)]
        max_pages: usize,
    },
    Preserve {
        file: PathBuf,
    },
    Compact {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        report: Option<PathBuf>,
    },
    Reconstruct {
        file: PathBuf,
    },
    Reconcile {
        file: PathBuf,
    },
    Amendment {
        law: String,
        base: String,
        file: PathBuf,
        #[arg(long)]
        source_url: String,
    },
}

#[derive(Subcommand)]
enum Export {
    Git {
        destination: PathBuf,
        #[arg(long)]
        law: Vec<String>,
        #[command(flatten)]
        time: Temporal,
    },
}

fn print<T: Serialize>(value: &T, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string(value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(value)?);
    }
    Ok(())
}

#[derive(Debug)]
struct ReportedFailure(&'static str);
mod error_output;
impl std::fmt::Display for ReportedFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for ReportedFailure {}

fn run(cli: Cli) -> anyhow::Result<()> {
    let today = japan_day(Utc::now())?;
    if let Command::Schema { output } = &cli.command {
        let bytes = serde_json::to_vec_pretty(&api::openapi())?;
        if let Some(path) = output {
            fs::write(path, bytes)?;
        } else {
            println!("{}", String::from_utf8(bytes)?);
        }
        return Ok(());
    }
    if let Command::Serve { bind, web } = &cli.command {
        let root = cli.store;
        Store::open(&root)?.rebuild()?;
        let router = api::router(api::ApiState { root, today: None }, Some(web.clone()));
        let address = *bind;
        let runtime = tokio::runtime::Runtime::new()?;
        return runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind(address).await?;
            eprintln!("law-history listening on http://{}", listener.local_addr()?);
            axum_serve(listener, router).await
        });
    }
    let store = Store::open(&cli.store)?;
    match cli.command {
        Command::Sync {
            all,
            law,
            catalog_only,
            import,
            bill,
            refresh,
            max_laws,
            max_requests,
        } => {
            if let Some(path) = import {
                return print(&ingest::import_bundle(&store, &fs::read(path)?)?, cli.json);
            }
            if let Some(path) = bill {
                return print(
                    &parliament::import_bill(&store, &fs::read(path)?, Utc::now())?,
                    cli.json,
                );
            }
            if !all && law.is_none() && !catalog_only {
                bail!("Choose --all, --law LAW_ID, --catalog-only, --import FILE, or --bill FILE");
            }
            let mut fetcher = Fetcher::new(EGOV_MINIMUM_DELAY, max_requests)?;
            let ids = if let Some(law) = law {
                vec![law]
            } else {
                let catalog = ingest::catalog(&store, &mut fetcher, refresh)?;
                if catalog_only {
                    return print(
                        &serde_json::json!({"catalog_laws":catalog.law_ids.len(),"reported_count":catalog.reported_count,"complete":catalog.complete,"collected_at":catalog.collected_at}),
                        cli.json,
                    );
                }
                catalog.law_ids
            };
            let mut report = ingest::SyncReport::default();
            for law in ids.into_iter().take(max_laws.unwrap_or(usize::MAX)) {
                let item =
                    ingest::sync_law(&store, &mut fetcher, &law, refresh).with_context(|| {
                        format!("Sync interrupted at {law}; rerun the same command to resume")
                    })?;
                eprintln!(
                    "{law}: saved {}, reused {}, diagnostics {}",
                    item.saved,
                    item.reused,
                    item.diagnostics.len()
                );
                report.laws += item.laws;
                report.saved += item.saved;
                report.reused += item.reused;
                report.diagnostics.extend(item.diagnostics);
            }
            print(&report, cli.json)?;
            if !report.diagnostics.is_empty() {
                return Err(ReportedFailure(
                    "Sync preserved source failures; inspect status and resume",
                )
                .into());
            }
        }
        Command::Search { text, time } => print(
            &service::search(&store, &text, &time.into(), today)?,
            cli.json,
        )?,
        Command::Log { law, time } => print(&store.history(&law, time.known_at)?, cli.json)?,
        Command::Show { law, time } => print(
            &service::select(&store, &law, &time.into(), today)?,
            cli.json,
        )?,
        Command::Diff { from, to } => print(&service::diff(&store, &from, &to)?, cli.json)?,
        Command::Blame { law, path, time } => print(
            &service::blame(&store, &law, &path, &time.into(), today)?,
            cli.json,
        )?,
        Command::Refs { law, time } => {
            print(&service::refs(&store, &law, &time.into(), today)?, cli.json)?
        }
        Command::Impact { from, to, time } => print(
            &service::impact(&store, &from, &to, &time.into(), today)?,
            cli.json,
        )?,
        Command::Simulate { file, save } => {
            let scenario: Scenario = serde_json::from_slice(&fs::read(file)?)?;
            let mut result = service::simulate(&store, &scenario)?;
            if save {
                let id = service::save_scenario(&store, &scenario, Utc::now())?;
                let saved: Scenario = store
                    .latest_fact(&format!("scenario:{id}"), None)?
                    .context("Saved scenario is unavailable")?;
                result = service::simulate(&store, &saved)?;
            }
            print(&result, cli.json)?;
            if !result.diagnostics.is_empty() {
                return Err(ReportedFailure("Simulation was refused; inspect diagnostics").into());
            }
        }
        Command::Scenario { id } => {
            let value: Scenario = store
                .latest_fact(&format!("scenario:{id}"), None)?
                .context("Scenario not found")?;
            print(&value, cli.json)?;
        }
        Command::Export {
            format:
                Export::Git {
                    destination,
                    law,
                    time,
                },
        } => {
            let files = service::export_snapshot(&store, &destination, &law, &time.into(), today)?;
            git_export::import_snapshot(&destination, &files, Utc::now())?;
            print(
                &serde_json::json!({"files":files.len(),"destination":destination}),
                cli.json,
            )?;
        }
        Command::Status => print(&service::status(&store, None)?, cli.json)?,
        Command::ImportCache { from } => print(
            &ingest::import_cache(&store, &Store::open(from)?, Utc::now)?,
            cli.json,
        )?,
        Command::Reindex => print(&serde_json::json!({"records":store.rebuild()?}), cli.json)?,
        Command::Verify => print(
            &serde_json::json!({"objects":store.verify_integrity()?}),
            cli.json,
        )?,
        Command::Compact { dry_run, report } => {
            let result = store.compact(dry_run)?;
            if let Some(path) = report {
                fs::write(path, serde_json::to_vec_pretty(&result)?)?;
            }
            print(&result, cli.json)?;
        }
        Command::Parliament { law, time } => {
            print(&service::parliament(&store, &law, time.known_at)?, cli.json)?
        }
        Command::Speeches {
            session,
            query,
            max_pages,
        } => {
            let mut fetcher = Fetcher::new(SPEECH_MINIMUM_DELAY, Some(max_pages))?;
            print(
                &serde_json::json!({"speeches":parliament::sync_speeches(&store, &mut fetcher, session, &query, max_pages)?}),
                cli.json,
            )?;
        }
        Command::Preserve { file } => print(
            &serde_json::json!({"sha256":store.preserve(&fs::read(file)?)?}),
            cli.json,
        )?,
        Command::Reconstruct { file } => print(
            &reconstruction::reconstruct(&store, &fs::read(file)?, Utc::now())?,
            cli.json,
        )?,
        Command::Reconcile { file } => {
            let result = reconstruction::record_comparison(&store, &fs::read(file)?, Utc::now())?;
            print(&result, cli.json)?;
            if !result.diagnostics.is_empty() {
                return Err(ReportedFailure("Official comparison contains mismatches").into());
            }
        }
        Command::Amendment {
            law,
            base,
            file,
            source_url,
        } => {
            let revision = store.version(&base)?;
            if revision.law_id != law {
                bail!("The base revision belongs to a different law");
            }
            let input = fs::read_to_string(file)?;
            let evidence = Provenance {
                url: source_url,
                raw_sha256: store.preserve(input.as_bytes())?,
                media_type: "text/plain or application/xml".into(),
                attribution: "Imported amendment source".into(),
                terms_url: revision.source.terms_url,
                confidence: Confidence::Inferred,
                locator: "source document".into(),
            };
            let result = amendment::parse_document(&input, &law, &revision.tree, &evidence);
            print(&result, cli.json)?;
            if !result.diagnostics.is_empty() {
                return Err(ReportedFailure(
                    "Amendment parsing was refused; inspect source diagnostics",
                )
                .into());
            }
        }
        Command::Serve { .. } | Command::Schema { .. } => unreachable!(),
    }
    Ok(())
}

async fn axum_serve(
    listener: tokio::net::TcpListener,
    router: law_history::api::RouterType,
) -> anyhow::Result<()> {
    law_history::api::serve(listener, router).await
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    if let Err(error) = run(cli) {
        if error_output::failure_output(json, error.downcast_ref::<ReportedFailure>().is_some())
            == error_output::FailureOutput::Json
        {
            println!("{}", serde_json::json!({"error":format!("{error:#}")}));
        } else {
            eprintln!("error: {error:#}");
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reported_failures_preserve_the_reason_and_select_the_output_channel() {
        use super::error_output::{FailureOutput, failure_output};
        assert_eq!(
            super::ReportedFailure("Source collection failed").to_string(),
            "Source collection failed"
        );
        for (json, reported, expected) in [
            (false, false, FailureOutput::Text),
            (false, true, FailureOutput::Text),
            (true, false, FailureOutput::Json),
            (true, true, FailureOutput::Text),
        ] {
            assert_eq!(failure_output(json, reported), expected);
        }
    }
}

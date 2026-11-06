# Use law-history

The local store defaults to `.law-history`.
The `--store` option selects a different directory for every command.
The `--json` option emits compact JSON.
Diagnostics preserve source URLs and locations.

## Collect public sources

```sh
mise x -- cargo run --package law-history-cli -- sync --catalog-only
mise x -- cargo run --package law-history-cli -- sync --law 321CONSTITUTION
mise x -- cargo run --package law-history-cli -- sync --all
```

The catalog includes every identifier returned by the paginated e-Gov law list.
Collecting the catalog doesn't collect each law's text.
The complete sync visits each law's revision list and downloads each available revision.
The collector reuses preserved responses within the collection UTC day.
Each cache response records completion of local preservation, and each parsed observation records its own processing time.
The collection command doesn't stamp a whole batch with its start time.
Unchanged revision metadata reuses the preserved revision on later days.
History and body revision metadata must agree before the collector saves a revision or receipt.
Compatible provided values fill missing or blank observations, while conflicting provided values remain diagnostics.
Merged revisions preserve the body response in `source` and the history response in `metadata_source`.
Response-specific update stamps remain separate and invalidate receipts when changed.
A scheduled date repeated as an enforcement placeholder agrees with an absent enforcement date and remains uncertain.
When cached body metadata disagrees, an available request budget permits one HTTP revalidation.
Persistent disagreement remains a source diagnostic.
The `--refresh` option fetches source responses again.
The `--max-laws` and `--max-requests` options bound a collection run.
Rerunning an interrupted command reuses its preserved sources and completed revision receipts.
Failed source requests remain in `status` and make the sync command fail.

The `import-cache --from ARCHIVE_STORE` command copies verified responses from a separate store into the selected destination store.
It imports only responses recorded on the current UTC day and keeps existing destination cache entries.
Each imported response receives a new local observation timestamp and an `archive-cache:` provenance record containing its previous recorded timestamp.
Import doesn't establish a new remote publication time or copy archived revision observations.
An imported response retains a recorded HTTP acquisition time.
When acquisition evidence is absent, only an HTTP revalidation can establish a dated current-state snapshot.
Rerunning `sync` parses the reusable responses and records fresh revision observations.
An archive with uncertain collection timestamps should remain separate from the active history index.

The [e-Gov API specification](https://laws.e-gov.go.jp/api/2/swagger-ui/lawapi-v2.yaml) defines the source fields.
Scheduled enforcement dates remain uncertain.
Dates before the Gregorian calendar transition keep their source notation until a source establishes the conversion.
An available consolidated revision isn't automatically an enactment revision.

## Select an enforcement and observation time

```sh
mise x -- cargo run --package law-history-cli -- search 日本国憲法
mise x -- cargo run --package law-history-cli -- show 321CONSTITUTION --at 1969-12-31
mise x -- cargo run --package law-history-cli -- log 321CONSTITUTION
mise x -- cargo run --package law-history-cli -- status
```

The `--at` option uses an enforcement calendar day.
The `--known-at` option uses an RFC 3339 observation timestamp.
Each revision retains the law's original promulgation in `promulgated` and any supplied amendment publication in `amendment_promulgated`.
An absent amendment publication date remains absent rather than inheriting the law's original date.
Selection uses the current date and all collected evidence when both options are absent.
An explicit `--version` inspects any preserved content address.
The explicit version may have a future or uncertain enforcement date.
Same-day revisions need a supported sequence or an explicit version for historical order.
A unique official current-state marker can select a matching revision for query days on or after its HTTP acquisition day in Japan Standard Time.
The marker doesn't establish a historical order and can't override contradictory sequence evidence.
The marker also pins the content address validated with the history metadata.
An undated repeal record produces an unknown-date diagnostic and needs an explicit version for inspection.
The details view shows termination evidence and a distinct link for complementary metadata provenance.

The `diff` command accepts two content addresses for the same law.
The `blame` command accepts a law identifier and a structured provision path.
The `refs` command reports recognized provision, definition, and delegation candidates with confidence and unresolved targets.
The `impact` command accepts the same content addresses as `diff` and separates direct changes from review paths through references.
An inferred reference is a review candidate.

## Apply and compare amendments

```sh
mise x -- cargo run --package law-history-cli -- amendment LAW_ID BASE_SHA256 amendment.xml --source-url https://example.org/source
mise x -- cargo run --package law-history-cli -- simulate scenario.json
mise x -- cargo run --package law-history-cli -- simulate scenario.json --save
mise x -- cargo run --package law-history-cli -- reconcile official-comparison.json
mise x -- cargo run --package law-history-cli -- reconstruct reconstruction.json
```

The amendment command accepts supported amending sentences or structured `AmendProvision` XML with `NewProvision` elements.
It recognizes article, paragraph, and item text substitutions, article deletion and renumbering, and structured article replacement or insertion.
Unrecognized input returns source locations and no inferred operations.
The shared `comparison_operations` function converts machine-readable comparison rows.

A scenario contains `schema_version`, `name`, `bases`, `at`, `known_at`, and `steps`.
Each base maps a law identifier to its SHA-256 revision address.
Each step has an identifier, an enforcement date, dependencies, sourced operations, and optional adjustments to a later step.
The generated [OpenAPI contract](../web/openapi.json) defines the exact JSON shapes.
The Web editor can create a scenario from a selected revision and save a shareable identifier through an explicit action.
The form names the target provision, enforcement date, original text, and replacement text.
The JSON editor rejects unknown scenario, step, and operation properties.
Saving fixes an omitted knowledge cutoff to the save time and pins the selected reference context in `reference_context`.
The saved context includes other-law version addresses and unresolved selection diagnostics.
Later imports can't add laws to that pinned context or change its selected versions.
Sharing identifies a scenario in the same store.
It doesn't publish the store to another machine.

An official comparison supplies `schema_version`, `scenario`, and `expected_versions` for every affected law.
A mismatch produces an `official_mismatch` diagnostic and doesn't create passing comparison evidence.
A reconstruction supplies `schema_version`, `scenario`, `revision_ids`, and preserved source `evidence`.
Reconstructed versions remain labeled as reconstructed.
They don't become official versions merely because an operation succeeds.
An enforcement-only scenario change also receives inferred provenance and a scenario revision identifier.

## Connect parliamentary evidence

```sh
mise x -- cargo run --package law-history-cli -- speeches --session 217 民法 --max-pages 1
mise x -- cargo run --package law-history-cli -- preserve bill-source.json
mise x -- cargo run --package law-history-cli -- sync --bill bill-linkage.json
mise x -- cargo run --package law-history-cli -- parliament 129AC0000000089
```

A bill's house, Diet session, category, and number identify the bill.
The category distinguishes Cabinet bills from member bills with the same number, following the [House of Representatives bill catalog](https://www.shugiin.go.jp/internet/itdb_gian.nsf/html/gian/kaiji217.htm).
Bill linkage imports include enacted law identifiers, submitters, a proposal reason, and speech identifiers with supporting sources.
A search result alone doesn't establish that a speech discusses a particular bill.
The [National Diet Library API](https://kokkai.ndl.go.jp/api.html) supplies speaker identities, speech text, and original speech URLs.
The collector sends sequential requests with at least three seconds between responses.
The stored speech includes source terms and speaker attribution.

## Export and recover

```sh
mise x -- cargo run --package law-history-cli -- export git /tmp/law-history-export --law 321CONSTITUTION --at 1969-12-31
mise x -- cargo run --package law-history-cli -- verify
mise x -- cargo run --package law-history-cli -- reindex
mise x -- cargo run --package law-history-cli -- compact --dry-run
mise x -- cargo run --package law-history-cli -- compact
```

The export destination must be empty.
The export contains selected text, provenance, and structured revision history in a local Git repository.
The generated commit uses its generation time.
Enforcement and observation times remain in the exported JSON.
Exporting doesn't push to a remote.

The `sources`, `versions`, and `records` directories contain the preserved corpus.
The `index.sqlite3` file is a derived search index.
Content addresses identify original decoded bytes, whether objects are raw or stored with a `.zst` suffix.
Compaction verifies restored bytes before removing a raw representation and removes duplicated tree payloads from SQLite.
The dry run reports potential object compression and the current index size in distinct fields.
The verifier checks content addresses, and rebuilding the index checks referenced source objects.

## Run checks

```sh
mise x -- just check
mise x -- just audit
mise x -- just proofs
mise x -- just fuzz
mise x -- just mutations --scope domain
mise x -- just mutations --list
mise x -- just mutations --shards 32 --jobs 6
mise x -- just release-check /path/to/complete-corpus
```

Use a release build for collection and queries on a large corpus: `mise x -- cargo run --release --package law-history-cli -- sync --all`.
The regular check covers Rust formatting, strict Clippy, regression tests, generated API freshness, TypeScript, the Web build, and browser workflows.
The mutation command defaults to the complete Rust runtime inventory.
Use `--file` for a selected source file or `--scope domain` for the pure core.
The `--list` option discovers candidates without claiming a tested result.
The `--shards` option partitions the complete inventory, and `--jobs` permits up to six independent bounded containers.
Each container permits 2 GiB of memory, two CPUs, 256 processes, and 900 seconds.
Mutation workers use separate temporary build caches and a private 512 MiB scratch mount within that memory limit.
Each shard still runs its unmutated baseline.
Mutation containers have no external network access, and local HTTP fixtures use loopback.
The regular native regressions use the host filesystem, while mutation scratch storage is volatile.
Time limits stop execution without supplying a passing verdict.
The native reports and source identities remain under `verification/results`.
The regular check also scans owned source files for secrets and requires its planted negative control to fail as expected.
The dependency audit uses the pinned RustSec database revision and a live npm advisory query.
The proof and fuzz commands build the pinned verification container and run it with 2 GiB memory, two CPUs, 256 processes, and a 900-second resource limit.
A resource stop leaves verification inconclusive.
The fuzz campaign uses a fixed seed and 10000 runs for each of the XML, legal-date, amending-document, object-codec, reference-resolution, and browser-snapshot targets.
The release check additionally requires complete source coverage and passing official amendment comparisons.
An empty or incomplete corpus can't pass the release check.

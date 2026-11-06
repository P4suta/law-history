# Project status

`law-history` is experimental software for inspecting Japanese national law history.
The command-line interface, `HTTP API`, and Web interface operate on a local store of preserved sources and structured revisions.
The source repository doesn't contain packaged releases or a complete law corpus.
The [public website](https://p4suta.github.io/law-history/) collects selected law histories into a browser corpus and runs the shared Rust services in WebAssembly.
The [browser guide](browser-corpus.md) covers collection, persistence, and transfer of pinned scenarios.

## Available workflows

The collector supports paginated e-Gov catalogs, available revision inventories, resumable retrieval, and source-preserving index reconstruction.
Enforcement days and observation timestamps support separate queries.
The app provides structured differences, recognized reference paths, amendment scenarios, parliamentary source linkage, and Git export.
The [usage guide](usage.md) defines the commands and the accepted amendment forms.

The demo imports synthetic records into `.law-history-demo`.
The normal collection store remains `.law-history`.
The Web demo is available at `http://127.0.0.1:3000` after the documented build and serve commands.

## Incomplete evidence

The project hasn't collected every available e-Gov law and revision.
The project hasn't completed full reconciliation of historical amendments with corresponding official revisions.
Available consolidated text doesn't establish the enactment text or recover a missing historical source.
Unknown enforcement dates, ambiguous amendment order, unrecognized syntax, and conflicting source claims remain diagnostics.

Parliamentary searches supply candidate speeches.
A bill linkage requires a preserved supporting record.
Successful simulation doesn't establish an official amendment or a legal interpretation.

## Verification and releases

`mise x -- just check` runs the fast source, API contract, and browser checks.
Continuous integration also checks workflow syntax and security with the pinned analyzers.
Proofs, fixed-seed fuzzing, and mutation testing have separate bounded commands.
The [assurance scope](assurance.md) records admitted domains, concrete bounds, external assumptions, counterexamples, and repaired verification gaps.
Passing checks don't establish the completeness of the legal model or the upstream archive.
The release gate additionally requires a complete corpus and recorded official amendment comparisons.
Incomplete source coverage can't produce a passing release verdict.

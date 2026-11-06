---
date: 2026-11-06
---

# Run shared history rules in the browser

## Status

Proposed.

## Context

[GitHub Pages](https://docs.github.com/en/pages/getting-started-with-github-pages/what-is-github-pages) serves static files.
The [e-Gov API](https://laws.e-gov.go.jp/api/2/laws?limit=1) permits browser requests through its cross-origin response policy.
The collected native store exceeds the [published site size limit](https://docs.github.com/en/pages/getting-started-with-github-pages/github-pages-limits).

## Decision

Run the shared Rust history, reference, and amendment implementation in a WebAssembly worker.
Preserve sources, versions, observations, and facts in browser storage with snapshot transfer.
Collect selected histories and display progress and coverage.
Scenario sharing pins bases and context.
Replay refuses unavailable versions.
Keep native behavior.

## Alternatives

A native server requires another host and ongoing operation.
A TypeScript implementation duplicates the history rules.

## Consequences

Storage limits and source availability bound collection and sharing.
Observation time records browser acquisition.

## Confirmation

Test generated WebAssembly under the deployment path with native API requests blocked.
Compare storage with SQLite and exercise corruption, interruption, and replay.
Run affected proofs, fuzzing, and mutation testing.
Reassess when central storage becomes necessary.

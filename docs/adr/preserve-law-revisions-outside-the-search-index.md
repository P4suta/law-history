---
date: 2026-11-05
---

# Preserve law revisions outside the search index

## Status

Accepted.

## Context

Enforcement dates and observation times describe different events and don't establish same-day amendment order.
[e-Gov](https://laws.e-gov.go.jp/docs/docs/ba4d819-laws-and-version-control/) describes staged enforcement, coordination, and corrections.
Recovery must preserve earlier evidence.

## Decision

Preserve source bytes, schema-versioned revisions, and observation records under SHA-256 addresses.
Rebuild SQLite offline from preserved records.
Selection compares indexed metadata with the hash-checked version.
Validated Gregorian days support years 1 through 9999 without Unix timestamps.
Preserve original notation and require evidence for historical calendar conversion.
CLI and HTTP share history services and pure domain decisions.
Scenarios pin bases and conditions and persist only through explicit save.
Multi-law failures return no result revisions, and saved reconstruction batches recover together.

## Alternatives

An authoritative index makes recovery depend on mutable state.
Git timestamps conflate generation and legal effect and exclude older dates.

## Consequences

Recovery needs immutable objects.
Uncertain dates, missing sources, and unordered amendments produce diagnostics.

## Confirmation

The [assurance checks](../assurance.md) cover recovery, temporal selection, atomic failures, and official text comparisons.
Reassess when persistence or temporal evidence contracts change.

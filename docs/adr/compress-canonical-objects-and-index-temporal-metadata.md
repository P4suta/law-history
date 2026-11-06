---
date: 2026-11-05
---

# Compress canonical objects and index temporal metadata

## Status

Accepted.

## Context

Duplicating historical trees in SQLite increases storage and coverage-query work.
Storage changes must preserve original bytes and content addresses.

## Decision

SHA-256 identifies decoded bytes.
Read raw digest files and `.zst` representations.
Use [Zstandard](https://docs.rs/zstd/0.14.3/zstd/) level 3 when smaller, with an 8 MiB decoding window and 256 MiB output limit.
Verify every read's hash and agreement between duplicate representations.
Compaction verifies compressed readback before removing raw bytes.
Interruption can leave both representations for resumption.
SQLite keeps temporal metadata and identifiers, and selection loads the preserved version.

## Alternatives

Raw storage avoids codec dependencies but consumes more space.
Keeping trees in SQLite duplicates preserved text and makes metadata queries read larger records.

## Consequences

Mixed representations remain readable and rebuildable without changing schemas or identifiers.
The codec and native dependency remain trust assumptions.
Corpus measurements don't predict unavailable historical material.

## Confirmation

The [assurance checks](../assurance.md) cover round trips, limits, corruption, interrupted representations, recovery, and codec fuzzing.
Reassess when codec limits or canonical object formats change.

---
date: 2026-11-05
---

# Verify immutable source snapshots and native outcomes

## Status

Accepted.

## Context

Long campaigns overlap with source edits.
Process success alone doesn't identify tested bytes or establish complete execution.
Resource failures can resemble compiler or test failures.

## Decision

Hash source, tests, fixtures, manifests, lockfiles, configuration, and verification code.
Execute exact immutable snapshots in bounded Linux containers.
Preserve inventories, native logs, phase results, tool versions, and source identities.
Every mutation shard must pass its baseline and complete its candidates.
Their union must match the initial inventory without duplicates and the unchanged working tree.
Behavioral detection requires native test failure.
Build rejection requires a Cargo compiler error and remains a separate count.
Survivors, inconsistent reports, and resource stops refuse completion.
Kani imports production decisions with declared assumptions and bounds.
A distinct false-assertion control must produce its expected counterexample.

## Alternatives

Serializing edits prevents drift but doesn't validate native outcomes.
Reusing process exits loses source and inventory evidence.

## Consequences

External behavior, legal interpretation, and string-search semantics remain distinct boundaries.

## Confirmation

The [assurance checks](../assurance.md) reject stale snapshots, missing work, resource errors, and incorrect proof controls.
Reassess when verifier semantics or execution formats change.

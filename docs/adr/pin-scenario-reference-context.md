---
date: 2026-11-05
---

# Pin scenario reference context

## Status

Proposed.

## Context

Saved impact analysis uses amendment bases and other laws' reference evidence.
Knowledge cutoffs alone can't freeze context after late backdated imports.

## Decision

Explicit save pins other laws' selected version addresses and diagnostics in `reference_context`.
An empty set fixes the absence of other laws.
Replay loads exactly those versions and validates their law identities and knowledge cutoff.
Amendment bases supply simulated laws, and repeated base laws are invalid.
Unsaved scenarios can omit context and use temporal selection.
The [scenario service](../../crates/law-history/src/service.rs) fills omitted context during save.

## Alternatives

Cutoffs alone permit late imports to change replay.
Copying source objects duplicates preserved content addresses.
Saving resolved references alone discards evidence needed to recompute analysis.

## Consequences

Pinned versions must remain available.
Context size grows with law count.
Reference interpretation and source completeness remain distinct from reproducibility.

## Confirmation

The [assurance checks](../assurance.md) replay after late corrections, added laws, and index recovery and reject invalid pinned identities.
Reassess when reference selection requires evidence beyond version addresses and diagnostics.

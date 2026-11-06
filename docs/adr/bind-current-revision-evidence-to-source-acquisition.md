---
date: 2026-11-05
---

# Bind current revision evidence to source acquisition

## Status

Proposed.

## Context

The [official API](https://laws.e-gov.go.jp/api/2/swagger-ui/lawapi-v2.yaml) identifies a revision in force at its source snapshot.
Same-day historical revisions can lack ordering evidence.
Cache replay supplies no fresh remote observation.

## Decision

Bind the unique current marker to the original HTTP acquisition time and matching body digest.
[Comparable metadata](merge-compatible-metadata-and-preserve-both-sources.md) must agree before publishing the inventory.
Selection requires the group with the greatest enforcement date and a query on or after the acquisition day in Japan Standard Time.
Knowledge cutoffs still apply.
Explicit sequence contradictions remain ambiguous.
Undated imported caches require HTTP revalidation before establishing snapshot freshness.
Receipt reuse validates law and revision subjects.

## Alternatives

Treating each parse as fresh admits stale snapshot evidence.
Inferring historical order from the current marker exceeds the source claim.

## Consequences

Current selection can use snapshot evidence while historical attribution remains conservative.
Missing acquisition metadata stays readable without establishing freshness.

## Confirmation

The [assurance checks](../assurance.md) cover date boundaries, cutoffs, corrections, cache replay, receipt subjects, and index recovery.
Reassess if upstream snapshot semantics change.

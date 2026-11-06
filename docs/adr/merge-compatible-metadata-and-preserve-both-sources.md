---
date: 2026-11-06
---

# Merge compatible metadata and preserve both sources

## Status

Proposed.

## Context

Official history and body responses supply complementary metadata with different update stamps.
The [API schema](https://laws.e-gov.go.jp/api/2/swagger-ui/lawapi-v2.yaml) describes their shared fields.

## Decision

Response-specific update stamps stay outside comparable metadata.
Null and blank values supply no claim.
Provided values fill missing observations.
Contradictory provided values fail.
Scheduled placeholders remain conditional dates.
Absent, uncertain, and known termination dates remain distinct, independently of status claims.
Processing dates don't establish legal termination dates.
Validated history expectations bind comparisons to preserved bytes and positions.
Merged revisions preserve body and history provenance, and storage verifies both sources.

## Alternatives

Whole-response equality rejects complementary dates.
Choosing one response loses evidence supplied by the other.

## Consequences

Dual provenance increases storage and readback work.
New evidence changes content addresses.
Legacy records remain readable.

## Confirmation

The [assurance checks](../assurance.md) cover preserved official pairs, nullable-field partitions, 625 enforcement-date pairs, ninety termination combinations, and production Kani decisions.
Reassess when upstream semantics change or official responses contradict the interpretation.

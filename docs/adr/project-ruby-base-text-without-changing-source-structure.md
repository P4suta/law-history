---
date: 2026-11-06
---

# Project ruby base text without changing source structure

## Status

Proposed.

## Context

Official history titles can be plain text while body metadata embeds ruby XML.
Literal equality rejects matching names.
Concatenating annotation text can also corrupt reference tokens.
The [official schema](https://laws.e-gov.go.jp/docs/law-data-basic/419a603-xml-schema-for-japanese-law/) distinguishes `Ruby` base content from `Rt` annotations.

## Decision

Keep original responses and complete element trees.
Use a separate base-text projection for title comparison and reference analysis.
The projection excludes annotation subtrees under `Ruby` and preserves other content.
A typed production transition governs annotation scope.
Pure attribute decisions require the element, name, and value independently.
Title fragments accept supported inline structure and reject unsupported or malformed markup.

## Alternatives

Ignoring every title difference conceals actual conflicts.
Concatenating all text includes readings.
Removing tags with a regular expression loses scope and parser validation.

## Consequences

Presentation annotations remain available in canonical objects and structural differences.
Projection doesn't establish legal meaning.

## Confirmation

Required checks include original responses, changed-base refusal, generated paths, schema-derived attribute combinations, reference counterexamples, fuzzing, and source-bound proofs.
Reassess when upstream inline semantics or supported structures change.

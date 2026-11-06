---
date: 2026-11-06
---

# Require witnesses for proof input domains

## Status

Proposed.

## Context

A Boolean model confused absent and uncertain termination dates.
Successful assertion summaries also concealed unreachable cover properties.

## Decision

Owned enum inputs derive symbolic generators from production types through [Kani's `Arbitrary`](https://model-checking.github.io/kani/crates/doc/kani/derive.Arbitrary.html).
Termination evidence keeps three date states independent of status claims and requires every input pair's witness.
The proof gate refuses unmet covers and requires exact counts for declared domain obligations.
Kani checks the production count decision over arbitrary optional machine-size inputs.
Independent source matrices assess adapter interpretation.

## Alternatives

Assertion-only summaries miss restricted domains.
Native examples don't quantify every admitted input.
Both checks complement symbolic witnesses.

## Consequences

Domain changes require inventory updates.
The gate depends on pinned verifier output and refuses malformed evidence.
Witnesses don't establish legal-model completeness.

## Confirmation

Thirty harnesses passed with required witnesses.
Restricting inputs left three of six witnesses unreachable and prevented completion despite successful assertions.
An added enum variant produced a counterexample.
The [assurance record](../assurance.md) retains the boundaries.
Reassess when source semantics, enum states, or verifier output change.

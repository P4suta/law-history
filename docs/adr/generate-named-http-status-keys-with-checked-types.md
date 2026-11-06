---
date: 2026-11-06
---

# Generate named response status keys with checked types

## Status

Proposed.

## Context

The pinned generator emits numeric HTTP response keys that fail the naming policy.
Quoting those keys changes TypeScript's numeric key types.

## Decision

Use the pinned generator's [Node API](https://openapi-ts.dev/node) and TypeScript abstract syntax trees.
Replace numeric members of response maps with computed names for literal constants.
Derive names from [Node's HTTP status descriptions](https://nodejs.org/api/http.html#httpstatus_codes).
Preserve property modifiers, optional tokens, and response types.
Check the actual API types and compare response-key types directly before writing output.

## Alternatives

Hand edits disappear on regeneration.
Quoted keys change `keyof`.
Text replacement can't identify response-map syntax reliably.

## Consequences

Generation remains deterministic with pinned Node, TypeScript, and generator versions.
The compiler checks the actual finite contract.
The compiler's type semantics remain a trust assumption.
The checks don't prove arbitrary syntax-tree transformation semantics.

## Confirmation

Require regeneration, response-key inventory, literal-value identity, type equality, invalid transformation controls, and Web workflows.
Reassess when the generated syntax or supported response-key forms change.

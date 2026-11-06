---
date: 2026-11-05
---

# Address provisions by unique number or sibling position

## Status

Proposed.

## Context

Versions must preserve source elements with repeated numbers.
An [official appendix](https://laws.e-gov.go.jp/api/2/law_data/324M50000080026_20250123_507M60000080001?law_full_text_format=xml) repeats `Item Num="1"` among siblings.

## Decision

Preserve every original `Num` attribute.
Use a numbered address only for a nonempty, nonzero, delimiter-free number unique among siblings with the same element name.
Otherwise use its ordinal sibling position.
Indexes, edits, and insertion locators share the decision.
Main-provision resolution checks ancestry through the production context transition.
An excluded context can't reenter valid scope.

## Alternatives

Rejecting repeated numbers discards valid source material.
Rewriting numbers invents identifiers.
Restricting ordinal addresses to known form names misses repeated appendix items.

## Consequences

Unique numbered addresses stay stable within a version.
Sibling changes can change addresses across versions, so every position includes its version.
Ambiguous legal numbers remain unresolved.

## Confirmation

The [assurance checks](../assurance.md) cover official fixtures, independent sibling enumeration, anchored ancestry grammar, and Kani context transitions.
Reassess when official XML introduces unsupported structural roles.

---
date: 2026-11-05
---

# Import generated law data with Git fast-import

## Status

Accepted.

## Context

Generated law data needs a generator identity without attributing legal text to a developer.
The [Git fast-import format](https://git-scm.com/docs/git-fast-import) supplies explicit committers and byte-counted data.

## Decision

Export selected text, history, provenance, selection conditions, and file digests into an empty destination.
The adapter accepts only `selection.json` and generated law directories' `text.txt`, `selected.json`, and `history.json` files.
Reject control paths, source paths, existing repository metadata, and changed file contents.
Use the `law-history export` identity and actual generation timestamp.
Require the `done` marker and index the imported tree.
Export configures no remote and publishes nothing.

## Alternatives

Ordinary developer commits can misattribute generated text.
Using enforcement timestamps conflates legal effect with repository generation.

## Consequences

Enforcement and observation dates remain in JSON.
Import failures leave an inspectable destination and report failure.
The adapter can't import arbitrary source files.

## Confirmation

Native integration tests clone generated repositories and check text, identities, timestamps, paths, and changed-content refusal.
Reassess when generated file contracts or Git import semantics change.

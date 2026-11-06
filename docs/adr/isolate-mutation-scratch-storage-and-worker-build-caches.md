---
date: 2026-11-06
---

# Isolate mutation scratch storage and worker build caches

## Status

Proposed.

## Context

Concurrent native baselines encountered test timeouts after successful builds.
Independent shard compilation also repeats dependency work.
Those observations don't identify every timeout's cause.

## Decision

Each worker owns a temporary [Cargo build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html) across sequential shards.
Every shard executes its native baseline.
Private 512 MiB [tmpfs scratch storage](https://docs.docker.com/engine/storage/tmpfs/) counts within the unchanged 2 GiB limit.
Evidence and build artifacts use host mounts.
External networking stays off.
Local HTTP fixtures use loopback.
Resource stops remain inconclusive.
Native logs use bounded reads and an inclusive 8 MiB admission limit.

## Alternatives

Host-backed scratch preserves isolation but retains shared storage contention.
Shared writable build directories introduce locking and mutation coupling.
Reusing baseline verdicts removes required execution evidence.

## Consequences

Cargo's artifact invalidation remains a toolchain assumption.
Volatile scratch supplies no power-loss persistence evidence.
Regular regressions still exercise the host filesystem.

## Confirmation

Cache, native-baseline, and diagnostic-limit controls check the adapter.
The [assurance record](../assurance.md) requires full candidate verification.
Reassess when resource stops recur or Cargo's invalidation contract changes.

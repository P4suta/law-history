---
date: 2026-11-06
---

# Use Linux-local mutation build storage

## Status

Proposed.

## Context

An unchanged baseline failed with [Rust error E0786](https://doc.rust-lang.org/error_codes/E0786.html) while opening dependency metadata on a host-backed build mount.
The filesystem or compiler cause remains unknown.
The [worker-cache decision](isolate-mutation-scratch-storage-and-worker-build-caches.md) leaves that host sharing boundary in the compilation path.

## Decision

Each mutation container receives a fresh anonymous [Docker volume](https://docs.docker.com/engine/storage/volumes/) for compiler output.
The volume uses the local driver without host filesystem mounts.
Source snapshots stay read-only, and results remain on the host.
Checked removal and error cleanup use [container-scoped volume removal](https://docs.docker.com/reference/cli/docker/container/rm/).
Invalid compiler metadata refuses mutation evidence instead of counting as a semantic compile rejection.

## Alternatives

Host-backed worker caches avoid repeated dependency builds but keep the failed boundary.
Shared named volumes require ownership and cleanup protocols.
Retrying the baseline leaves the observed dependency unchanged.

## Consequences

Shards rebuild dependencies within existing limits.
Docker and the compiler remain external trust assumptions.
Compiler output no longer depends on host sharing.

## Confirmation

Contracts require local storage and owned cleanup.
Native controls reject E0786 while retaining ordinary compiler diagnostics.
The full mutation inventory must finish with every baseline and owned cleanup complete.
Reassess the storage boundary if metadata failures recur on local volumes.

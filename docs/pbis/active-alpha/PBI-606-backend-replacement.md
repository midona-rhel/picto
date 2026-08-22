# PBI-606: Backend Replacement

## Problem

Picto currently implements the same library behavior through overlapping database facades, engines,
compiler/change-impact layers, detailed settlement events, and duplicated ingest/subscription paths.
SQLite can commit before projections settle, so the frontend can briefly display items and counts
that are not true for its active scope. The flattened media experiment also cannot represent ordered
multi-media source posts as one user-visible item.

## Required Behavior

- A root item is standalone media or an ordered collection. Attached members inherit the root's
  lifecycle and folders.
- `All` contains active roots only; Inbox and Trash never leak into accepted-library scopes/counts.
- Every mutation is one SQLite transaction, synchronous projection settlement, one revision, and one
  compact invalidation receipt.
- Every query, count, selection, and export resolves the same root set.
- Every import source enters one durable ingest path. Subscriptions use one persisted run worker and
  resume non-terminal source items after restart.
- Physical bytes are reused by hash without collapsing distinct logical source occurrences.
- Replaced backend paths, cloud sync, fake verification, and pass-through tests are deleted.
- The current active development library is converted once at the reviewed cutover point. The
  conversion is manual, backup-first, never shipped, and deleted after verification.

## Acceptance

The executable checklist is `docs/RELEASE_COMPLETION_PLAN.md`. This PBI closes only after the
packaged fresh-library smoke passes all five user-verification points. Delete this file when it
closes; Git history is the archive.

# PBI-528: Bitmap flush crash recovery verification

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The crash safety concern is theoretical — based on the 30-second flush interval and the absence of explicit crash recovery tests. If the bitmap rebuild-from-SQLite path has been tested manually, the risk may be lower than stated. This PBI is primarily about adding automated verification.

## Priority
P2

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: Bitmaps flush every 30 seconds via the background worker in `core/src/workers.rs`. Between flushes, mutations are in-memory only. The manifest stores the last-published bitmap file versions in SQLite. On library open, `open_with_active_file()` in `core/src/sqlite/bitmaps.rs` loads from the manifest payload. If the manifest is stale (crash between mutation and flush), bitmaps will be inconsistent with the SQLite source of truth until the next compiler run.

## Problem
Between bitmap flushes (every 30 seconds), all bitmap mutations exist only in memory. If the application crashes (OOM, force-quit, power loss):

1. SQLite data is safe (WAL + `PRAGMA synchronous = NORMAL` protects committed transactions)
2. Bitmap data may be up to 30 seconds stale
3. On next library open, the stale bitmaps load from the manifest
4. Sidebar counts, smart folder results, and tag membership may be incorrect until the next compiler cycle runs

The rebuild path exists (compilers can recompute all bitmaps from SQLite), but there is no automated test verifying that crash recovery actually produces correct results. There is also no mechanism to detect that bitmaps are stale relative to SQLite data.

## Scope
- `core/src/sqlite/bitmaps.rs` — flush and load paths
- `core/src/sqlite/compilers.rs` — RebuildAll path
- `core/src/sqlite/open.rs` — library open sequence
- `core/src/workers.rs` — flush worker interval

## Implementation
1. **Add epoch consistency check on library open**: After loading bitmaps from the manifest, compare the manifest's published epoch with the database's current state. If they diverge (e.g., file count in SQLite doesn't match Status bitmap cardinality), trigger a `RebuildAll`.
2. **Write integration test**: Test that simulates a crash (skip final flush) → reopen library → verify bitmaps match SQLite after automatic rebuild.
3. **Consider flush-on-mutation for critical operations**: For operations that change many files (bulk import, bulk delete), flush bitmaps immediately after the operation completes rather than waiting for the 30-second timer. This narrows the crash window for the most impactful operations.

## Acceptance Criteria
1. Library open detects stale bitmaps and triggers rebuild automatically.
2. After crash-recovery rebuild, sidebar counts match actual SQLite data.
3. Integration test covers: insert files → skip flush → reopen → verify bitmap consistency.
4. No regression in normal (non-crash) library open performance.

## Test Cases
1. Insert 100 files → force-skip the flush timer → reopen library → Status(1) bitmap cardinality matches file count in SQLite.
2. Tag 50 files → force-skip flush → reopen → EffectiveTag bitmaps match entity_tag_raw table.
3. Normal shutdown (flush completes) → reopen → no rebuild triggered (bitmaps are fresh).
4. Bulk import 500 files → verify immediate flush occurs after import completes.

## Risk
Low-Medium. The epoch consistency check must be fast (single COUNT query + bitmap cardinality comparison). If it triggers unnecessary rebuilds on every open, it defeats the purpose of bitmap caching. A lightweight heuristic (file count + last-modified timestamp) should suffice.

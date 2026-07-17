# PBI-602: Multi-device sync architecture (append-only oplog over dumb object storage)

## Priority
P2

## AI-generated caveat
This document locks the architecture contract for syncing one library across multiple devices through a user-owned storage backend (Google Drive first). It is grounded in a 2026-07-17 audit of the live schema (`core/src/db/core/schema.rs`, v100) and write paths. It intentionally locks *invariants and data contracts*, not implementation sequencing — the staged rollout at the end is guidance, not scope.

## Lifecycle
- `Implemented` when the contract below (remote layout, op vocabulary, merge rules, identity rules) is final and the op-log write path exists behind a flag.
- `Activatable` when PBI-601 is activated (local durability is a hard dependency).
- `Activated` when a second device can replay a library end-to-end and converge.
- `Legacy removed` — n/a (additive system).

Activation depends on:
- [PBI-601-local-persistence-crash-safety-hardening.md](./docs/pbis/active-alpha/PBI-601-local-persistence-crash-safety-hardening.md)

## Problem
A live SQLite database cannot be placed in a file-sync folder: WAL's three-file consistency, mid-transaction snapshots, and whole-file conflict resolution make corruption or silent whole-session loss inevitable. Yet users want one library on multiple devices, synced through storage they already own (Google Drive), with no possibility of one device destructively interfering with another, and graceful behavior when metadata arrives before media.

The current architecture is unusually well-suited to solving this — content-addressed immutable blobs, hash identity at the API boundary, and a strict truth-vs-derived split enforced by the compiler system — but three things are missing: a syncable representation of truth (today it exists only inside SQLite), stable cross-device identities for several entity kinds, and separation of truth from operational state in two mixed tables.

## Product model
The design goal is stated as three guarantees, in order:

1. **Impossible by construction** — the remote store contains only immutable, write-once objects in single-writer namespaces. Partial mutation, merge conflicts at the storage layer, and cross-device overwrites structurally cannot occur.
2. **Detected if it happens anyway** — checksums, hash verification, and convergence digests turn any violation into a loud alarm, never silent drift.
3. **Recoverable when detected** — local SQLite is a disposable materialized view; remote truth is append-only and never mutated. The worst realistic bug (a replay/merge defect) is fixed by shipping code and rebuilding local state.

## Locked decisions

### 1. Backend interface is four operations; Google Drive is an implementation detail
The sync layer talks to a trait with exactly: `put(key, bytes)` (immutable, create-only), `get(key)`, `list(prefix)`, `delete(key)` (GC only). Google Drive API is the first backend (`files.create`, `changes.list` polling); Dropbox, S3, and WebDAV are known-compatible. Nothing above the trait may depend on backend-specific behavior. Backends without change feeds poll `list`.

### 2. Remote layout
```
<remote-root>/
  blobs/f/<ab>/<cd>/<hash>.<ext>     immutable originals, content-addressed
  oplog/<device-id>/<seq>.jsonl      immutable op segments, per-device namespace
  status/<device-id>.json            small mutable-per-device status object (only exception to immutability; single-writer)
  snapshots/                         optional future checkpoint objects (versioned, immutable)
```
- Only device X ever writes under `oplog/X/` and `status/X.json` — single writer per prefix, no contention by construction.
- Segments are numbered, immutable, and uploaded whole. There is no append: a segment exists completely or not at all. (This is why torn-tail handling is unnecessary remotely.)
- Thumbnails do not sync; they are derived and regenerated locally.

### 3. Op records: framed, checksummed, versioned, idempotent
- Each segment is a sequence of length-prefixed, CRC-checked records plus a segment-level checksum. A damaged segment is quarantined, never half-applied.
- Every op carries: `op_version`, op type, hybrid logical clock (HLC), `device_id`, per-device `seq`, and payload keyed by **stable identity** (below).
- Ops are idempotent under replay.
- **Forward-compatibility rule:** an op with an unknown `op_version` or type is *parked* — the reader stops advancing its cursor past it and surfaces "update required". Unknown data is never dropped.
- The op vocabulary is derived from the mutating dispatch command surface (entities, tags, folders, smart folders, collections, duplicates-decisions, subscription/group config, groups). Deletions are tombstone ops; the log never contains destructive rewrites.

### 4. Deterministic total order and convergence
- Replay order is `(HLC, device_id, seq)` — a total order independent of arrival order. Same segment set ⇒ byte-identical truth state on every device.
- Merge rules: scalar truth fields (status, rating, name, notes, predicate_json, config fields) merge last-writer-wins per `(entity, field)` by HLC. Set memberships (entity↔tag, folder members, collection members) merge as add/remove pairs with LWW per pair.
- `entity_tag` merges **per `(entity, tag, source)`**, respecting the existing provenance model (`source` is in the PK; `provenance_mask` per PBI-598). A user tag and a subscription-sourced tag are different facts and never clobber each other.
- Determinism is enforced by property test: shuffle segment arrival order, assert identical state digest.

**Delete vs concurrent edit: edits resurrect (add-wins).** An op targeting a tombstoned entity revives it (device A deletes a collection while device B concurrently adds a member ⇒ after sync, the collection exists everywhere and contains B's addition). Rationale: asymmetry of harm — a wrongly-kept container is visible and costs seconds to re-delete; a wrongly-dropped edit silently discards user intent, which this design exists to prevent. Both rules converge identically under the total order; this is purely semantics. Two boundaries:
- Resurrection is surfaced, not silent: the revived entity is flagged in the UI ("deleted on <device>, restored because changes arrived from <device>") so the user makes the final call.
- Only *concurrent* edits resurrect. A device that has already replayed the tombstone cannot edit the deleted entity in the first place — no conflict exists.
- Scalar conflicts (two devices rename the same entity) remain plain LWW: both expressed intent about the same field, someone must win, nothing structural is lost.

### 5. Stable identities
- Files: `media_file.file_hash`. Entities: `media_entity.entity_hash` (already exists). Tags: `(namespace, subtag)`.
- **New UUIDs required** for: `folder`, `smart_folder`, `subscription`, `subscription_group`, and collections (collection entities without a content hash). Local dense integer PKs never appear in ops. `folder.watch_path` is not an identity.
- Device identity: random UUID minted per install, stored locally outside the library.
- **Restore-from-backup rule:** on startup, if `list(oplog/<device-id>/)` shows a remote seq ≥ the local next-seq, the device's counter has regressed (restored from backup); the device must mint a fresh device ID. A same-name segment overwrite must never be possible.

### 6. Durability and upload ordering
Per mutation: (1) blob written atomically + fsync'd locally (PBI-601), (2) op appended to the local durable outbox (fsync'd), (3) user sees success, (4) blob uploaded, (5) segment referencing it uploaded. Blobs strictly precede the segments that reference them, shrinking the metadata-before-media window to in-flight upload races only.

### 7. Missing-blob handling (the closed-laptop case)
A replayed op may reference a blob not yet present (uploader offline, Drive placeholder not hydrated). This is a *normal state*, not an error:
- The entity renders with a "pending sync" placeholder.
- A reconciliation task retries: wait for backend sync → fetch from remote → if still absent, re-download from the entity's recorded source URLs.
- Any recovered bytes are accepted **only** if they hash to the expected `file_hash`. Exact verification — no fuzzy metadata matching.

### 8. What syncs, per the live schema
- **Truth (syncs):** `media_entity` (minus cached aggregates), `media_file` identity columns, `single_media_entity`, `entity_tag`, `tag` (`namespace`,`subtag`,`site_mask`), `tag_alias`, `tag_implication`, `folder` + `folder_member`, `smart_folder` predicates, subscription/group **config**, and the *decision* half of `duplicate` (`status`, winner/loser by hash, `decision_*`).
- **Derived (never syncs, rebuilt locally):** bitmaps, `tag_ancestor`, `entity_tag_implied`, `sidebar_node`, `file_color`(+rtree), FTS, all cached counters (`tag.file_count`, `media_entity.member_count`/`total_size_bytes`, folder sizes), `perceptual_hash`/color analysis columns, detected duplicate pairs.
- **Device-local (never syncs):** `subscription_query` operational columns (`resume_cursor`, `last_check_time`, `files_found`, failure state), all run history (`subscription_run`, `subscription_query_run`, `subscription_query_job`, `subscription_issue`, `subscription_download_attempt`, `subscription_post_member`), `ingest_queue*`, `deferred_work_item`, `view_pref`, pin state, and **credentials — `credential_domain`/`credential_health` and keychain secrets never leave the machine**.
- **Mixed-table surgery required:** `duplicate` (sync decisions, recompute detections), `subscription_query` (sync config columns only), cached aggregate columns excluded from ops everywhere.
- Subscription scheduling across devices: no coordination in this slice — if two devices run the same subscription, content-addressed dedup makes it wasteful, not harmful. A designated-fetcher refinement is a future concern.

### 9. Verification machinery
- Segment CRCs (decision 3) checked on every read.
- Blob hash verified on upload and on first download; periodic background scrub re-hashes local blobs and repairs from remote or source URLs on mismatch.
- Each device's `status/<device-id>.json` publishes a digest (hash of canonical truth state) plus per-peer high-water marks. Two devices with the same segment coverage and different digests ⇒ convergence alarm ⇒ rebuild-from-log, surfaced to the user.
- Remote scrub: a device noticing a blob referenced by the log but absent remotely re-uploads it if held locally; otherwise the entity is marked missing-at-source for the reconciliation task.

### 10. Deletion and GC
- User deletion is a tombstone op; blobs are not deleted remotely as a side effect.
- Remote blob GC is a separate, explicit, user-invoked operation: compute refcounts from the fully replayed log, delete only blobs unreferenced by any non-tombstoned entity, with a grace period. GC is the only caller of backend `delete` for blobs.

### 11. Local SQLite is never in the synced folder
The library root splits: synced root (blobs + oplog + status) vs. local root (SQLite, thumbnails, queues, outbox). Opening a library from a synced root alone must fully materialize the local root by replay.

### 12. The cloud is never the sole copy — devices are full replicas
Decided 2026-07-17: the remote store may not hold the only copy of any blob. Every device maintains a full local blob replica (eager hydration of blobs referenced by replayed ops; the missing-blob placeholder of decision 7 is a transient state, not a storage policy). Thin caches / local eviction are explicitly out of scope. Consequences:
- Provider loss (account lockout, ban, service shutdown) is never data loss — any device can re-populate a fresh backend from scratch.
- Devices without capacity for the full library are not supported in this slice.

### 13. No client-side encryption in this slice
Decided 2026-07-17: remote objects are stored unencrypted. Encryption is a possible future addition, not current scope. To keep that door open cheaply:
- All remote reads/writes already pass through the four-op backend trait — an encrypting wrapper around that trait is the future insertion point.
- If encryption is ever added, remote object names must switch from plaintext hashes to keyed HMACs (plaintext-hash filenames enable confirmation attacks), and the entire archive re-uploads. This cost is accepted.
- Until then: the provider can read library content, and any device can browse blobs via the provider's own UI.

## Open policy questions (not locked)
1. **Second-backend mirroring.** Largely defused by decision 12 (any device can rebuild a backend), but "archive" positioning may still want an automatic second remote. Deferred.
2. **Client-side encryption.** Deferred (see decision 13). Feasible on all target providers; costs full re-upload plus key-custody UX whenever adopted.
3. **Log compaction.** Segments accumulate forever; replay time and item counts grow. Snapshots (decision 2) become mandatory at archive timescales. Note: Google Drive enforces an account item-count cap (on the order of millions of files) — content-addressed sharding plus segments consumes it steadily.

## Acceptance criteria
- [ ] Backend trait with the four operations; Google Drive implementation; unit-tested against a local-filesystem fake backend.
- [ ] Op vocabulary covering the mutating command surface for all truth tables in decision 8, with tombstones, versioning, framing, and CRCs.
- [ ] UUID identity added to folders, smart folders, collections, subscriptions, groups; no dense ID ever serialized into an op.
- [ ] Dual-write: every truth mutation emits an op to the durable outbox in the same logical action as the SQLite write.
- [ ] Full rebuild: a fresh device pointed at a synced root materializes a complete, correct library from blobs + oplog alone.
- [ ] Determinism property test (shuffled segment arrival ⇒ identical digest) passing.
- [ ] Convergence digest published and compared; mismatch surfaces an alarm.
- [ ] Missing-blob placeholder + reconciliation (remote fetch → source re-download → hash-verify) working end-to-end.
- [ ] Restore-from-backup device-ID regression check on startup.
- [ ] Credentials and all device-local tables verifiably absent from every remote object.

## Testing requirements
- Unit: framing/CRC round-trip, torn-segment quarantine, HLC ordering, merge rules per field class, provenance-respecting tag merge.
- Property: replay determinism under shuffled arrival; idempotent double-replay.
- Integration: two simulated devices over the fake backend — concurrent tag edits, concurrent status changes, delete-vs-edit races (assert resurrection + surfaced flag), offline device catching up, blob-arrives-late, seq-regression detection.
- Backend fault injection: rate limiting (429), quota exhausted mid-upload, stale/duplicated list results, upload succeeding after reported timeout — assert the outbox retries safely and never double-writes a segment name.
- Manual: real Google Drive account, two machines, import on A → appears on B; kill A mid-upload → B shows pending placeholder → recovers.

## Implementation notes (staged, non-binding)
1. Stage 1 — identity + outbox: UUIDs, op emission dual-write, no network. Ships silently.
2. Stage 2 — replay engine + fake backend + determinism tests. Local-only.
3. Stage 3 — Drive backend, single-writer multi-reader.
4. Stage 4 — full multi-writer with convergence digests, scrub, GC.

### Stage 1 progress (2026-07-17)
Foundations landed:
- `core/src/oplog.rs`: `new_uuid()` (32-hex identity), `next_hlc()` (monotonic hybrid logical clock), `device_id()` (stable per-install id under `~/.picto/device-id`, outside any library root; TODO: move to Electron app-data once the host passes it through initialize), `record_op()` (outbox insert), `OP_VERSION = 1`.
- `op_outbox` table in the live schema + open-time reconcile. Op rows are written inside the same `with_write` transaction as the mutation they describe — the dual-write is atomic by construction (relies on PBI-601).
- `uuid` columns with unique indexes on `folder`, `smart_folder`, `subscription`, `subscription_group`; generated on insert, backfilled for existing rows on open.
- Reference emission pattern wired for the folder domain: `folder_created` / `folder_updated` (truth fields only — watch config is device-local) / `folder_moved` / `folder_deleted` (tombstone), keyed by folder uuid.

Remaining for Stage 1: emission wiring for the other truth domains (entities/status/metadata, tags + tag graph, smart folders, collections, subscription/group config, duplicate decisions) following the folder pattern; entity/tag ops key on `entity_hash` / `(namespace, subtag)` and need no new identity. Note: `db/write/subscriptions.rs` is dead code (dispatch routes through `runtime_service`) — delete rather than wire.

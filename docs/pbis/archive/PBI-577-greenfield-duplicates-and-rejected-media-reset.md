# PBI-577: Greenfield duplicates reset

## Priority
P1

## Completed (2026-08-14)

The duplicate backend and rebuilt review surface are active. The packaged Electron review smoke at
`scripts/ci/duplicate-review-smoke.mjs`. Direct preload IPC is used only to seed fixtures and read
authoritative backend state. The review flow itself navigates the rendered app to Duplicates,
clicks visible `Scan library` and `Re-scan library` controls, waits for both rendered candidate
cards, clicks visible `Keep both`, and verifies the rendered empty state. It then relaunches the
same library, navigates to Duplicates again, verifies the rendered empty state, and compares it
with persisted backend counts. The packaged run passed against
`dist/mac-arm64/Picto.app`; its report is `artifacts/duplicates/smoke.json`.

The smoke also verifies the product boundary: active media is counted by `All`, Inbox is counted
separately, and neither is silently treated as the other. The companion
`scripts/ci/duplicate-scan-benchmark.mjs` measures the native scanner against a user-supplied
library and records observed file count, pHash population, candidate count, and elapsed time. It
makes no claim about an unmeasured library size.

The scanner no longer performs an unconditional all-pairs comparison. Its exact partitioned
Hamming index matches brute force on deterministic data and edge thresholds. A 4,096-hash release
measurement recorded 240 candidates, 13.57 ms brute force, and 20.00 ms indexed. That small-data
measurement proves parity and records overhead; it does not claim a speedup at unmeasured scale.

Ingest-time duplicate lookup no longer scans every stored pHash for every imported image. The
canonical schema stores eight indexed 32-bit partitions per valid 256-bit pHash for the normal
97% review threshold, then verifies full Hamming distance on the indexed candidates. Candidate
lookup, media insertion, index maintenance, and duplicate-pair insertion share one serialized
SQLite write transaction. A one-million-row SQLite query-plan probe used all eight indexes and
returned its single synthetic candidate in about 1 ms; this is an index probe, not an end-to-end
one-million-item import benchmark. Less strict custom thresholds retain the exact full-scan
fallback rather than silently changing results.

Duplicate review intentionally covers Active and Inbox media, while Trash is excluded immediately
from scan sources, review pages, and counts. This does not change `All`: it remains Active-only.

## Evidence gate

Run after packaging:

```sh
npm run alpha:package
node scripts/ci/duplicate-review-smoke.mjs --dist dist --report artifacts/duplicates/smoke.json
node scripts/ci/duplicate-scan-benchmark.mjs --library /path/to/representative.library --report artifacts/duplicates/scan-benchmark.json
```

The harness proves accessible rendered controls and text state through CDP DOM evaluation, not
pixel-level visual fidelity. Collection-owner choices, reference repointing, candidate guards,
all five decisions, physical-hash cleanup, cleanup retry, and restart recovery are covered by the
focused Rust tests. Duplicate notifications and linked comparison behavior are covered by focused
frontend tests.

## Lifecycle
- `Implemented` when duplicates exist as one bounded canonical subsystem on the new model.
- `Activatable` when `PBI-567`, `PBI-568`, and `PBI-576` are implemented, and `PBI-573` is implemented where exact file-hash reuse and exact/near pHash behavior are part of ingest.
- `Activated` when the live duplicate path uses the new subsystem by default.
- `Legacy removed` when replaced duplicate/review paths for that activated slice are deleted.

The database, engine, ingest, background worker, and rendered review surface are live. The removed
`find_similar` command, response types, and unused public similarity path are not retained for
compatibility.

## Problem
The current duplicates system is still too close to the old file-table implementation and not yet aligned tightly enough with the canonical entity/file model.

Current problems:
- duplicate scanning and pair review must remain file-based while review decisions operate on owning entities
- resolution evidence must cover references, collection ownership, and physical blob cleanup
- scan performance must be measured on a representative library before optimization claims are made

## Product model to encode
The duplicates/review subsystem should reflect these truths:
- duplicate similarity is computed on files
- review outcomes affect entities because entities are what the user sees
- one physical file has one single entity
- that single entity belongs to at most one collection
- there is no global rejected-media database
- exact file hash means same media and should reuse the existing entity
- exact pHash is only relevant for comparable static images
- near pHash similarity should create duplicate review work, not auto-reject

## Locked decisions

### 1. Duplicate similarity is file-based
Perceptual-hash and exact duplicate logic operate on `media_file`, not on collection rows or generic entity rows.

That means:
- `perceptual_hash` belongs to the file layer
- duplicate candidates are file-to-file candidates
- review and resolution then map back to the owning single entities

### 2. One representative file, one representative single entity
When resolving duplicates, the system picks a representative winner at the file level.

Because one file maps to one single entity:
- keeping the winner file means keeping its single entity
- losing files cause their single entities to be removed or merged away
- folders, collections, subscriptions, and other references are repointed to the winner single entity as needed

### 3. Collection ownership conflicts must be explicit during duplicate resolution
If duplicate resolution touches entities that belong to different collections:
- the system must not silently duplicate entities
- the system must not silently keep both collection owners
- the system must require an explicit choice about which collection keeps the surviving entity

Do not hide collection ownership conflicts inside duplicate resolution.

### 4. Exact file hash means reuse
If an inbound file hash matches an existing file hash:
- do not create a new file row
- do not create a new single entity
- reuse the existing single entity and merge metadata/context onto it

### 5. Exact pHash is image-only and conservative
Exact pHash comparison is only meaningful for comparable static raster images.

That means:
- no image-vs-video superiority logic
- no cross-type auto-replace logic
- exact pHash may auto-resolve only when the new image is clearly better
- if quality is ambiguous, import it and send it to duplicate review

### 6. Near pHash means review, not suppression
If an inbound item is near-duplicate by pHash:
- import it normally
- insert/update duplicate review pairs
- do not auto-merge
- do not auto-reject

## Required subsystem shape

### Main categories
The subsystem should expose:
- duplicate scanning
- duplicate pair review
- representative selection / resolution
- exact-match upgrade heuristics for comparable static images

### Persisted records
The new model should include explicit records such as:
- duplicate candidate / pair rows
- duplicate review decision rows

Exact names can improve, but the domain split is required.

## Boundaries
- Similarity and exact-match work remain file-based.
- Review decisions operate on the owning media entities.
- Ingest owns exact-hash reuse and duplicate-candidate creation.
- Background work may compute pHash; duplicate review owns the resulting decision.

## Acceptance criteria
This PBI is complete only when:
- duplicate similarity is clearly file-based
- representative selection and entity repointing are explicit
- collection ownership conflicts are explicit and reviewable
- exact file hash reuses the existing single entity
- exact pHash auto-resolution is image-only and conservative
- exact pHash ambiguity goes to review
- near pHash creates review work instead of auto-merging
- there is no rejected-media database or rejected lifecycle status

## Verification checklist

Completed evidence:

- packaged Electron smoke covers rendered scan, re-scan, candidate loading, Keep Both, sidebar
  settlement, active-versus-Inbox counts, and restart persistence
- indexed candidate generation matches brute force and retains dense identical-hash results
- focused tests cover every destructive and non-destructive decision
- focused tests cover explicit cross-collection owner choice and reference repointing
- physical loser cleanup is transactionally queued, attempted immediately, and retried after
  failure or restart through the durable work queue
- ordinary deletion and orphan sweeping use the same per-hash lease, live-reference recheck, and
  durable cleanup queue, so a same-hash reimport cannot lose its blob
- full TypeScript, Vitest, Rust, command parity, formatting, package, and diff gates pass

## Tests
Required tests:
- duplicate scan over files with perceptual hash
- duplicate resolution repoints references to the winner single entity
- duplicate resolution across different collections requires an explicit owner choice
- exact file hash import reuses the existing single entity
- exact pHash static image with clearly better new image auto-resolves to the better version
- exact pHash static image with ambiguous quality imports and creates a duplicate-review pair
- near pHash match imports and creates a duplicate-review pair
- cross-type media never uses exact pHash superiority logic

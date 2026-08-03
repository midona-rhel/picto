# PBI-577: Greenfield duplicates reset

## Priority
P1

## Current audit status (2026-08-03)

Backend and rebuilt manager implemented; app smoke remains. Canonical ingest computes pHash,
exact hash reuse exists, exact/near pHash behavior has tests, duplicate decisions are persisted,
and cross-collection resolution requires an explicit owner. The rebuilt frontend now has a typed
duplicate API, paginated comparison screen, scan/loading/error/empty states, all five decisions,
keyboard navigation, and explicit collection-owner conflict handling. `system:duplicates` routes
through the manager surface instead of the unavailable placeholder.

Do not add another duplicate controller or global workspace store unless a second consumer needs
one. The screen owns transient review state and calls the typed API directly. Close and archive
this PBI after an Electron smoke pass proves scanning, media loading, each decision, sidebar count
refresh, and the collection-conflict dialog against a real library.

## AI-generated caveat
This document is based on an in-repo audit of the current duplicate scanner, perceptual-hash code, duplicate-pair storage, and manual review flow. It intentionally excludes a rejected-media database because the product model does not want one.

## Lifecycle
- `Implemented` when duplicates exist as one bounded canonical subsystem on the new model.
- `Activatable` when `PBI-567`, `PBI-568`, and `PBI-576` are implemented, and `PBI-573` is implemented where exact file-hash reuse and exact/near pHash behavior are part of ingest.
- `Activated` when the live duplicate path uses the new subsystem by default.
- `Legacy removed` when replaced duplicate/review paths for that activated slice are deleted.

The database, engine, ingest, and background-work foundations are live. Their historical plans
are archived and are no longer scheduling dependencies.

## Problem
The current duplicates system is still too close to the old file-table implementation and not yet aligned tightly enough with the canonical entity/file model.

Current problems:
- duplicate scanning and pair review are still strongly file-table-shaped
- duplicate resolution and entity reuse rules are not aligned tightly enough with the canonical ingest model
- exact file-hash reuse, exact pHash upgrade, and near-pHash review decisions were not previously one coherent contract

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

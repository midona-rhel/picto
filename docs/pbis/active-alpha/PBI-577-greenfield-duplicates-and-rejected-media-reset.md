# PBI-577: Greenfield duplicates and rejected-media reset

## Priority
P1

## AI-generated caveat
This document is based on an in-repo audit of the current duplicate scanner, perceptual-hash code, duplicate-pair storage, and manual review flow. It also incorporates the product requirement to retain and surface rejected media and exact rejected fingerprints.

## Problem
The current duplicates system is too close to the current file-table implementation and does not yet model rejected-media behavior explicitly enough.

Current problems:
- duplicate scanning and pair review are still strongly file-table-shaped
- duplicate resolution and entity reuse rules are not aligned tightly enough with the new entity/file model
- rejected media is not modeled as a first-class reviewable domain
- exact rejected fingerprints are not persisted strongly enough to suppress future exact repeats

## Product model to encode
The duplicates/review subsystem should reflect these truths:
- duplicate similarity is computed on files
- review outcomes affect entities because entities are what the user sees
- one physical file has one single entity
- that single entity belongs to at most one collection
- rejected media remains visible in a dedicated rejected scope
- exact rejected fingerprints should suppress future exact repeats

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

### 4. Rejected media is a first-class domain
Rejected media must be persisted explicitly.

The model should include:
- rejected media rows
- rejection reason / source
- review timestamps
- links to the underlying entity/file where applicable
- exact-match suppression fingerprints

### 5. Only exact rejected fingerprint matches auto-reject
The product requirement is exact suppression, not fuzzy suppression.

That means:
- exact file hash matches may auto-reject
- exact perceptual-hash matches may auto-reject if the perceptual hash is an exact match
- near duplicates must not be auto-rejected just because they are similar

### 6. Rejected media is queryable
The engine view model should expose a dedicated rejected scope so the user can review previously rejected items.

## Required subsystem shape

### Main categories
The subsystem should expose:
- duplicate scanning
- duplicate pair review
- representative selection / resolution
- rejected-media review
- exact rejected fingerprint maintenance

### Persisted records
The new model should include explicit records such as:
- duplicate candidate / pair rows
- duplicate review decision rows
- rejected media rows
- exact rejected fingerprint rows

Exact names can improve, but the domain split is required.

## Relationship to other reset PBIs
- PBI-567 defines one file to one single entity
- PBI-573 ingest must consult exact rejected fingerprints during import
- PBI-568 must expose rejected scope as a normal entity-view/system scope
- PBI-576 may schedule perceptual-hash computation and similar work for this subsystem

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This PBI is complete only when:
- duplicate similarity is clearly file-based
- representative selection and entity repointing are explicit
- collection ownership conflicts are explicit and reviewable
- rejected media is a first-class persisted domain
- exact rejected fingerprints suppress future exact repeats
- rejected media is viewable through the normal query/view system

## Tests
Required tests:
- duplicate scan over files with perceptual hash
- duplicate resolution repoints references to the winner single entity
- duplicate resolution across different collections requires an explicit owner choice
- exact rejected file hash suppresses re-import
- exact rejected perceptual hash suppresses re-import
- near-duplicate perceptual hash does not auto-reject
- rejected scope returns persisted rejected items

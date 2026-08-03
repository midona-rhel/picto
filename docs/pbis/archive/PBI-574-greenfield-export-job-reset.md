# PBI-574: Greenfield export job reset

## Priority
P1

## AI-generated caveat
This document is based on an in-repo audit of the current export commands and media I/O helpers. It is intentionally narrow. Export is treated as a job domain, not as an ad hoc file helper.

## Lifecycle
- `Implemented` when export exists as a real job boundary starting from `EntityTarget`.
- `Activatable` when `PBI-568`, `PBI-569`, and `PBI-578` are implemented enough for export to use canonical targets and media delivery rules.
- `Activated` when the live export path uses the new export job boundary by default.
- `Legacy removed` when replaced ad hoc export/media-I/O paths for that activated slice are deleted.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-569-greenfield-media-delivery-service.md](./docs/pbis/active-alpha/PBI-569-greenfield-media-delivery-service.md)
- [PBI-578-bulk-entity-target-and-selection-reset.md](./docs/pbis/active-alpha/PBI-578-bulk-entity-target-and-selection-reset.md)

## Problem
Export is currently mixed into media I/O helpers and shaped too much like file operations instead of one entity-targeted job flow.

Current problems:
- export is still coupled to path-shaped helpers and one-off commands
- bulk export semantics are not clearly unified with the rest of the entity-target model
- collection export behavior is not locked explicitly enough
- progress, cancellation, and output accounting are job concerns, but the current surface is thinner than that

## Product model to encode
The export layer should reflect these truths:
- export is a long-running job
- export starts from an entity target, not from file-table plumbing
- export of a collection usually means exporting its members, not a fake “collection file”
- progress and output accounting are part of the job contract

## Locked decisions

### 1. Export is a job surface
The main export API should be shaped like:
- `start_export_job(target, spec)`
- `get_export_job(job_id)`
- `cancel_export_job(job_id)`

Do not keep per-file export as the primary public contract.

### 2. Export consumes `EntityTarget`
Export must accept the same bulk-target model as the rest of the engine:
- `entity_hashes`
- `query_results`

Do not maintain a separate selection-only export surface.

### 3. Export defaults to descendant media for collections
For file-producing export, a collection target should export its member media by default.

That means export uses an explicit expansion policy, with:
- default: `DescendantsOnly`
- optional future override if the product later adds cover-only or manifest export modes

### 4. Export is media-delivery aware
Export should use the media delivery and asset-resolution model where appropriate.

Do not rebuild a second path-resolution abstraction inside export.

## Required export shape

### Export spec
The export request should carry:
- output destination
- output mode such as original vs converted
- optional conversion format
- optional resize / quality rules
- optional structure mode for collections and folders

### Export result
The export job must report:
- total targeted entities
- total exported files
- skipped files
- suppressed / unavailable items
- conversion errors
- written output paths or output manifest location

## Relationship to other reset PBIs
- PBI-568 defines the engine boundary that starts export
- PBI-569 defines the media-delivery and asset-resolution model export should build on
- PBI-578 defines the bulk-target model export must reuse
- PBI-576 defines the shared background job/deferred-work model export should align with

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This PBI is complete only when:
- export is a job-oriented engine surface
- export consumes the same `EntityTarget` model as the rest of the engine
- collection export semantics are explicit
- export no longer depends on path-shaped public helpers as its main contract
- progress, cancellation, and structured results are part of the export API

## Tests
Required tests:
- start/cancel/query export job flow
- single-entity export
- query-results export without enumerating all hashes in the request
- collection export expands to member media by default
- export handles unavailable assets explicitly
- conversion and original-copy modes both report correct structured results

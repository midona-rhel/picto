# PBI-573: Greenfield import and ingest reset

## Priority
P1

## AI-generated caveat
This document is based on an in-repo audit of the current import pipeline, watch-folder path, and subscription-import path. It is intentionally decisive. The implementing engineer should simplify further where that preserves the same ingest model.

## Lifecycle
- `Implemented` when one ingest pipeline/service exists for incoming media.
- `Activatable` when `PBI-567`, `PBI-568`, and `PBI-576` are implemented, and `PBI-577` is implemented where reject/suppress behavior is required.
- `Activated` when manual import and the intended automated ingest paths use the shared ingest pipeline by default.
- `Legacy removed` when replaced one-off import paths for that activated slice are deleted.

Activation depends on:
- [PBI-567-greenfield-library-database-reset.md](./docs/pbis/active-alpha/PBI-567-greenfield-library-database-reset.md)
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-576-greenfield-deferred-work-and-background-processing-reset.md](./docs/pbis/active-alpha/PBI-576-greenfield-deferred-work-and-background-processing-reset.md)
- [PBI-577-greenfield-duplicates-and-rejected-media-reset.md](./docs/pbis/active-alpha/PBI-577-greenfield-duplicates-and-rejected-media-reset.md) where reject/suppress behavior is part of the activated flow

## Problem
The application still has multiple ingestion paths that conceptually do the same thing but are shaped around different callers and historical shortcuts.

Current problems:
- manual import, folder-watch import, and subscription import still feel like adjacent systems instead of one ingest pipeline
- import logic still leaks file-centric structure upward instead of presenting one media-entity outcome
- duplicate/reuse behavior is not locked tightly enough at the ingest boundary
- reject/suppress behavior is not integrated into the main ingest decision path
- deferred heavy work is still mixed with ingest instead of being scheduled cleanly

This PBI defines one ingest pipeline for every source of incoming media.

## Product model to encode
The ingest layer should reflect these truths:
- every incoming media file enters the library through one ingest service
- ingest creates or reuses `media_file`
- ingest creates or reuses the matching single `media_entity`
- the same single entity can then be attached to folders and subscriptions, and to at most one collection
- the import source changes policy and metadata, not the storage model
- heavy follow-up work is scheduled, not hidden inside the ingest path

## Locked decisions

### 1. One ingest pipeline
All inbound media uses one ingest service:
- manual file import
- manual folder import
- watch-folder import
- subscription import
- any future remote ingest

Do not keep separate storage semantics for different import sources.

### 2. One file maps to one single entity
If the same physical file is seen again:
- reuse the existing `media_file`
- reuse the existing single `media_entity`
- attach that existing entity to any requested collections/folders/subscriptions

Do not create duplicate single entities for the same file.

### 3. Import source is explicit
Each ingest request carries a source classification such as:
- `manual`
- `watch_folder`
- `subscription`
- `migration`

The source may affect:
- initial status
- default folder attachment
- default collection behavior
- policy decisions
- audit and review UI

It must not create a separate storage model.

### 4. Auto-collection is a grouping policy, not a second ingest system
When the source supplies grouping information:
- the ingest pipeline imports/reuses member entities first
- then attaches those members to a collection according to one collection-grouping rule

Do not maintain a separate “subscription collection import” storage path.

### 4a. Collection ownership conflicts are explicit
If ingest wants to place a single entity into a collection but that entity already belongs to a different collection:
- ingest must not silently duplicate the entity
- ingest must not silently move the entity
- ingest records a collection-ownership conflict for review

Moving an entity between collections must be an explicit action.

### 5. Reject/suppress checks happen inside ingest
Before a new inbound item becomes a normal inbox/active entity, ingest must consult:
- exact duplicate reuse rules
- exact rejected-media suppress rules from the duplicates/rejected-media reset

If an inbound item exactly matches a previously rejected fingerprint:
- ingest records the attempt
- the item lands in rejected review state instead of normal inbox flow

### 6. Deferred heavy work is scheduled, not hidden
Ingest may synchronously do only the minimum required to create correct stored rows and basic visible results.

Heavy work such as:
- thumbnail generation
- preview frame generation
- dominant color extraction
- perceptual hash computation
- AI tagging

must be handed off to the deferred-work system.

## Required ingest shape

### Main engine surface
The engine should expose one ingest-facing surface such as:
- `start_import_job(request)`
- `get_import_job(job_id)`
- optional `cancel_import_job(job_id)`

Convenience wrappers may exist for UI ergonomics, but they should collapse into one ingest job model.

### Ingest request
The ingest request should explicitly carry:
- source type
- source paths or source-produced staged files
- initial status policy
- optional folder attachment
- optional collection grouping hints
- optional source URLs / source metadata
- optional tag hints

### Ingest outcome
The ingest result should distinguish:
- created files
- reused files
- created entities
- reused entities
- created collections
- collection attachments
- rejected/suppressed items
- scheduled deferred work

Do not reduce ingest outcomes to one vague imported/skipped count.

## Relationship to other reset PBIs
- PBI-567 defines the canonical storage model for files, entities, and collections
- PBI-568 defines the engine boundary above ingest
- PBI-576 defines the deferred-work system ingest should schedule into
- PBI-577 defines exact duplicate and rejected-media behavior ingest must consult

PBI-573 is the ingest and source-normalization layer sitting on top of those.

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This PBI is complete only when:
- all inbound media uses one ingest pipeline
- file reuse and entity reuse are explicit and correct
- manual import, watch-folder import, and subscription import share the same ingest semantics
- auto-collection is handled as a grouping policy inside ingest, not a separate storage path
- rejected exact matches are consulted during ingest
- deferred heavy work is scheduled through the background-work system instead of being hidden inside ingest
- ingest outcomes are structured enough for UI, review, and state-change publication

## Tests
Required tests:
- manual import create vs reuse roundtrip
- watch-folder import through the same ingest path
- subscription import through the same ingest path
- collection-group import attaches members without creating duplicate single entities
- same file imported twice reuses the same single entity
- collection ownership conflicts are recorded instead of silently duplicating or moving an entity
- exact rejected fingerprint routes to rejected state instead of normal inbox
- ingest schedules deferred work instead of requiring inline derivative completion

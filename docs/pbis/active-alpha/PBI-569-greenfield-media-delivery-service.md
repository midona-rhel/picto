# PBI-569: Greenfield media delivery service

## Priority
P1

## AI-generated caveat
This document is based on an in-repo audit of the current media I/O and viewer surface plus product intent clarified during review. It is intentionally concrete and decision-complete, but it is still AI-generated planning. The implementing engineer should simplify further where that preserves the same delivery API.

## Lifecycle
- `Implemented` when `MediaDeliveryService` exists with typed asset roles and stable delivery outputs.
- `Activatable` when `PBI-568` is implemented enough for the engine to call media delivery and `PBI-570` is implemented enough for the frontend to consume media URLs/handles.
- `Activated` when viewer/grid/preview surfaces use the media delivery service by default.
- `Legacy removed` when path-shaped media helpers for the activated slice are deleted.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-570-greenfield-frontend-reset-program-index.md](./docs/pbis/active-alpha/PBI-570-greenfield-frontend-reset-program-index.md)

## Problem
The current backend media surface is still path-oriented and transport-shaped instead of being a clean media delivery service.

Current problems:
- the public API still exposes file-path resolution as a first-class behavior
- thumbnail, original-media, preview, and file-opening concerns are split across separate commands
- the frontend still depends on path-shaped or implementation-shaped helpers instead of one typed asset model
- video delivery is not modeled as a dedicated streaming concern
- collection media behavior is spread across frontend assumptions instead of one backend rule
- media delivery is not yet isolated behind a stable service API that could survive transport or hosting changes

This PBI is the media-delivery companion to the database and backend engine reset. The database PBI defines canonical stored data. The engine PBI defines query and write behavior. This PBI defines how media assets are actually delivered to the frontend.

## Product model to encode
The media delivery layer should reflect these application truths:
- the frontend should consume stable backend-generated media URLs or URL-like handles
- filesystem paths are backend internals
- the product works with asset roles, not with ad hoc path helpers
- videos need a real streaming-capable delivery path from the backend
- collections resolve all asset roles through one consistent primary-member rule
- deferred derivative generation is separate from media delivery reads
- media delivery is a dedicated abstraction layer, not a side effect of storage or engine internals

## Locked decisions

### 1. One media delivery service
Use one backend media delivery service that owns:
- thumbnails
- preview images
- original media
- video streaming

Do not keep separate public services as the main API for:
- file path resolution
- thumbnail path resolution
- original-vs-thumbnail lookup

Locked rule:
- media delivery is its own backend service boundary, not a helper bag hanging off the engine or the library database

### 2. Stable media URLs or handles
The frontend consumes stable backend-generated URLs or URL-like handles.

That means:
- the backend decides blob/file lookup
- the frontend does not need filesystem paths
- the backend can change physical storage layout without changing the frontend contract

Those URLs or handles are the abstraction boundary. They must remain valid regardless of whether the media is served by a local process, a remote backend, or a different storage layout later.

### 2a. Media delivery may keep its own delivery-side storage
The media delivery service may keep its own storage for delivery concerns when needed.

Examples:
- asset URL/handle state
- stream/session state
- delivery cache manifests
- derivative-serving metadata that is not part of the canonical library model

Rules:
- canonical library facts still come from `LibraryDatabase`
- delivery-specific state belongs to `MediaDeliveryService`
- the engine should not need to know whether delivery state is stored in SQLite, files, or another store

### 3. One typed asset role model
Use one asset role system:
- `thumbnail`
- `preview_image`
- `original_media`
- `video_stream`

These roles are product concepts, not storage concepts.

### 4. One consistent collection primary-member rule
For collections:
- `thumbnail`
- `preview_image`
- `original_media`
- `video_stream` when applicable

must all resolve through one selected primary-member rule.

The delivery layer is not allowed to pick different member files for different roles unless the product later introduces an explicit separate rule.

Locked rule:
- the primary member is the current first member by ordinal
- that choice is materialized on the collection row as `primary_member_entity_id`

### 5. Video playback requires streaming support
Video delivery must support range/stream semantics from the backend.

The implementation may be HTTP/range or an equivalent backend transport, but the public contract must behave like a real stream-capable media endpoint.

### 6. Deferred work is not part of normal asset reads
Thumbnail generation, preview-frame generation, color extraction, phash generation, and related background work are not part of the normal asset-read API.

Asset reads should:
- return current availability
- return structured absence when missing

Maintenance and deferred processing stay in their own APIs.

## Public API

### Asset descriptor
Use one typed asset result such as:

```ts
type EntityAssetResult = {
  role: 'thumbnail' | 'preview_image' | 'original_media' | 'video_stream';
  available: boolean;
  url?: string;
  mime?: string;
  source_entity_hash?: string;
};
```

### Main API
Use one main public API such as:
- `resolve_entity_asset(entity_hash, role)`
- optional convenience wrapper `get_entity_asset_url(entity_hash, role)` if the frontend benefits from a URL-only helper

The public API must not be shaped like:
- `resolve_file_path`
- `resolve_thumbnail_path`
- separate original-media path getters as the primary frontend API

## Asset behavior

### Single image
- `thumbnail` = generated thumbnail if present
- `preview_image` = original image
- `original_media` = original image file
- `video_stream` = unavailable

### Single video
- `thumbnail` = generated thumbnail
- `preview_image` = generated thumbnail or best preview frame
- `original_media` = original video file
- `video_stream` = stream-capable video endpoint

### Collection
- `thumbnail` = primary member thumbnail
- `preview_image` = primary member best display image
- `original_media` = primary member original media
- `video_stream` = primary member stream endpoint if the primary member is a video, otherwise unavailable

The same primary member must be used for all roles.

## Implementation changes
- add a dedicated backend media delivery module/service separate from the engine and separate from thin transport adapters
- make blob/file lookup private to that service
- add a real stream-capable video delivery path
- define stable URL or handle generation there
- route viewer/grid/preview asset usage through the delivery API instead of path helpers
- move thumbnail regeneration and deferred derivative backfill out of the public delivery read API; keep them as maintenance or deferred-work APIs

## Relationship to PBI-568
This PBI is intentionally split out from PBI-568.

PBI-568 defines:
- backend query and write behavior
- entity-facing command and read APIs
- `ApplicationEngine` as the main backend behavior layer above storage

PBI-569 defines:
- how media assets are delivered and streamed
- how the frontend obtains thumbnails, previews, originals, and streams

Do not bury media delivery inside generic engine commands or transport adapters. It is its own backend service boundary that the engine and transport layer can call.

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This PBI is complete only when:
- the frontend no longer needs direct filesystem paths
- the public media API is role-based and entity-centric
- thumbnails, previews, originals, and video streaming are owned by one delivery service
- collections resolve all roles consistently through one primary-member rule
- missing assets return structured absence instead of vague path errors
- media delivery is exposed through one stable service API rather than path-shaped helpers
- deferred work is not mixed into the main asset-read API

## Tests
Required tests:
- single-image asset resolution
- single-video asset resolution
- collection asset resolution via primary member
- unavailable role returns structured absence
- stable URL/handle API tests
- video streaming/range behavior tests
- boundary test proving frontend no longer depends on direct path resolution

## Adjacent cleanup expected during implementation
While implementing this PBI, also remove:
- public path-oriented media commands
- duplicated original-vs-thumbnail helper surfaces
- frontend helpers that assume filesystem-path access

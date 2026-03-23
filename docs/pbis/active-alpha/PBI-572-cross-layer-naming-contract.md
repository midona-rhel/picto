# PBI-572: Cross-layer naming rules

## Priority
P1

## AI-generated caveat
This document is based on the reset PBIs and the current shared API surface. It is intentionally prescriptive. The goal is to remove naming drift, not to preserve historical wording.

## Problem
The project still carries multiple names for the same concept across database, backend, frontend, and transport code.

Current problems:
- the same concept appears as `hash`, `entity_hash`, `file_hash`, or other partial names depending on the layer
- user-visible time fields still drift between `imported_at`, `created_at`, `updated_at`, `date_added`, `date_created`, and `date_modified`
- some public DTO names still reflect legacy implementation details instead of product concepts
- some internal storage names still leak upward into backend and frontend code

This PBI locks one naming system across the reset architecture so the database, backend, media delivery, and frontend all talk about the same model the same way.

This is a prerequisite PBI for the greenfield reset set. Read and apply it before implementing the other reset PBIs in this series.

This PBI must also follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Naming goals
The naming system must make these things true:
- one concept has one canonical name
- public names describe the product model, not the storage history
- database names, backend names, and frontend names line up closely enough that moving between layers is obvious
- storage-only details stay storage-only
- public contracts do not leak implementation-specific ids or paths unless they are true product concepts

## Cross-layer rules

### 1. Use semantic names, not historical names
Use names that describe what the thing is now.

Do not keep names like:
- `imported_at` when the product means `date_added`
- `hash` when the product means `entity_hash`
- `is_collection` when the product means `entity_kind`
- `slim` when the product means a specific projection such as `EntityGridItem`

### 2. Use one naming style per layer
- database tables and columns: `snake_case`
- Rust types: `PascalCase`
- Rust fields and serialized API fields: `snake_case`
- TypeScript types: `PascalCase`
- TypeScript API field names: exactly the serialized backend field names in `snake_case`

Do not translate backend field names into a second frontend vocabulary for shared DTOs.

### 3. Internal ids are internal unless they are real product identifiers
Use:
- `entity_hash` as the public stable media-entity identifier
- `file_hash` as the physical file content identifier

Treat these as internal-only by default:
- `entity_id`
- `file_id`

They may exist inside the database and backend internals, but they should not be required by the public frontend contract unless there is a very strong reason.

### 4. User-visible domain rows use domain date names
For user-visible domain objects, use:
- `date_added`
- `date_created`
- `date_modified`

Do not use:
- `created_at`
- `updated_at`

for those same meanings.

`created_at` and `updated_at` are reserved for operational or infrastructure records only, and even there explicit names are preferred when they are easy to define.

Examples:
- media entity: `date_added`, `date_created`, `date_modified`
- folder: `date_added`, `date_modified`
- smart folder: `date_added`, `date_modified`
- subscription: `date_added`, `date_modified`
- deferred work: `queued_at`, `started_at`, `finished_at`, `last_error_at`

### 5. Public DTOs must not leak storage structure
Public DTOs should say what the frontend needs to know, not how the database happens to store it.

Examples:
- use `entity_kind`, not `is_collection`
- use `member_count`, not `collection_item_count`
- use media asset URLs/results, not `thumbnail_hash` or filesystem paths
- use `EntityGridItem`, not `EntitySlim`

## Canonical vocabulary

### Core nouns
Use these names everywhere:

| Concept | Database | Backend public | Frontend public |
| --- | --- | --- | --- |
| top-level media thing | `media_entity` | `MediaEntity` internally, projected as `EntityGridItem` / `EntityDetails` | `EntityGridItem` / `EntityDetails` |
| physical file | `media_file` | internal `MediaFile` only | not a first-class frontend concept |
| single-to-file bridge | `single_media_entity` | internal only | not exposed |
| collection ownership fields | `parent_collection_entity_id`, `collection_ordinal` | internal only, surfaced through entity projections | not exposed directly |
| folder | `folder` | `FolderNode`, `FolderDetails`, or equivalent | same |
| folder membership | `folder_member` | internal only | not exposed directly |
| smart folder | `smart_folder` | `SmartFolderNode`, `SmartFolderDetails`, or equivalent | same |
| tag | `tag` | `TagInfo`, `TagSearchResult`, `TagDetails` | same |
| tag alias | `tag_alias` | internal + admin/query DTOs | same where exposed |
| tag implication | `tag_implication` | internal + admin/query DTOs | same where exposed |
| deferred work item | `deferred_work_item` | `DeferredWorkItem` / `DeferredWorkSummary` | same |

### Identity fields
Use these exact names:

| Meaning | Database | Backend public | Frontend public | Notes |
| --- | --- | --- | --- | --- |
| stable entity identity | `entity_hash` | `entity_hash` | `entity_hash` | primary public media identifier |
| physical file content identity | `file_hash` | `file_hash` only where truly needed | avoid exposing by default | mostly backend/storage concern |
| internal entity row id | `entity_id` | internal only unless unavoidable | avoid exposing | not a stable product identifier |
| internal file row id | `file_id` | internal only | not exposed | storage-only |
| folder id | `folder_id` | `folder_id` | `folder_id` | public until a stronger folder identity exists |
| smart folder id | `smart_folder_id` | `smart_folder_id` | `smart_folder_id` | public until a stronger smart-folder identity exists |
| tag id | `tag_id` | `tag_id` | `tag_id` where needed | acceptable public admin/query identifier |

### Entity fields
Use these exact names:

| Meaning | Database | Backend public | Frontend public |
| --- | --- | --- | --- |
| entity kind | `entity_kind` | `entity_kind` | `entity_kind` |
| display name | `name` | `name` | `name` |
| status | `status` | `status` | `status` |
| rating | `rating` | `rating` | `rating` |
| notes | `notes` or `notes_json` if stored as JSON text | `notes` | `notes` |
| source urls | `source_urls_json` if stored as JSON text | `source_urls` | `source_urls` |
| date entity entered library | `date_added` | `date_added` | `date_added` |
| original creation/publication date | `date_created` | `date_created` | `date_created` |
| last user-visible metadata modification | `date_modified` | `date_modified` | `date_modified` |
| number of collection members | `member_count` | `member_count` | `member_count` |
| collection total size | `total_size_bytes` | `total_size_bytes` | `total_size_bytes` where exposed |
| owning collection | `parent_collection_entity_id` | internal only | not exposed directly |
| order inside collection | `collection_ordinal` | internal only | not exposed directly |
| collection primary member | `primary_member_entity_id` | internal resolver field, not frontend DTO | not exposed directly |

### File and media-analysis fields
Use these exact names:

| Meaning | Database | Backend public | Frontend public |
| --- | --- | --- | --- |
| mime type | `mime_type` | `mime_type` | `mime_type` |
| byte size | `size_bytes` | `size_bytes` | `size_bytes` |
| width | `pixel_width` | `pixel_width` | `pixel_width` |
| height | `pixel_height` | `pixel_height` | `pixel_height` |
| duration | `duration_ms` | `duration_ms` | `duration_ms` |
| frame count | `frame_count` | `frame_count` | `frame_count` |
| audio present | `has_audio` | `has_audio` | `has_audio` |
| perceptual hash | `perceptual_hash` | `perceptual_hash` only where needed | usually not exposed |
| dominant color | `dominant_color_hex` | `dominant_color_hex` | `dominant_color_hex` |

These are intentionally stronger than generic `mime`, `size`, `width`, and `height`.

### Projection type names
Use these exact type names:
- `EntityGridItem`
- `EntityDetails`
- `EntityViewQuery`
- `EntityViewPage`
- `EntityTarget`
- `MediaEntityPatch`
- `EntityAssetResult`
- `SelectionSummary`
- `DeferredWorkSummary`

Do not use:
- `EntitySlim`
- `GridPageSlimQuery`
- `GridPageSlimResponse`
- `GridOutlineResponse`

### Public command names
Use these exact names:
- `query_entity_view`
- `get_entity_details`
- `get_entity_grid_items`
- `patch_media_entities`
- `set_entity_status`
- `apply_entity_tags`
- `update_folder_membership`
- `resolve_entity_asset`
- `get_entity_asset_url`
- `get_selection_summary`
- `get_deferred_work_summary`
- `retry_deferred_work`

`patch_media_entities` is preferred over `update_media_entities` because the payload is explicitly partial.

### Asset names
Use these exact asset roles:
- `thumbnail`
- `preview_image`
- `original_media`
- `video_stream`

Use these exact result field names:
- `role`
- `available`
- `url`
- `mime_type`
- `source_entity_hash`

Do not expose:
- `path`
- `thumbnail_hash`
- `original_path`
- `thumbnail_path`

### Query-shape names
Use these exact names in the public query model:

| Meaning | Public name |
| --- | --- |
| base view scope | `base_scope` |
| system scope key | `key` |
| optional filters | `filters` |
| sort config | `sort` |
| page config | `page` |
| next cursor | `next_cursor` |
| total result count | `total_count` |

Within `EntityViewQuery.filters`, use:
- `rating`
- `colors`
- `mime_types`
- `tags`
- `date_created`
- `date_added`
- `date_modified`

Within `EntityViewQuery.sort`, use:
- `field`
- `direction`

Within `EntityTarget`, use:
- `kind: 'entity_hashes'`
- `kind: 'query_results'`
- `entity_hashes`
- `query`
- `excluded_entity_hashes`

Do not use a separate public “selection” target shape for bulk entity write operations.

## Naming migration map
These renames are required during the reset:

| Old / weak name | Canonical name |
| --- | --- |
| `hash` | `entity_hash` |
| `kind` | `entity_kind` |
| `is_collection` | `entity_kind === 'collection'` |
| `collection_item_count` | `member_count` |
| `imported_at` | `date_added` |
| `created_at` on domain rows | `date_created` or `date_added`, depending on meaning |
| `updated_at` on domain rows | `date_modified` |
| `mime` | `mime_type` |
| `size` | `size_bytes` |
| `width` | `pixel_width` |
| `height` | `pixel_height` |
| `num_frames` | `frame_count` |
| `phash` | `perceptual_hash` |
| `EntitySlim` | `EntityGridItem` |
| `GridPageSlimQuery` | `EntityViewQuery` |
| `GridPageSlimResponse` | `EntityViewPage` |
| `thumbnail_hash` in public DTOs | remove; use media delivery URL/result |
| `update_media_entities` | `patch_media_entities` |
| `path` in public asset results | `url` |

## Relationship to reset PBIs
This naming contract is mandatory for:
- [PBI-567-greenfield-library-database-reset.md](./docs/pbis/active-alpha/PBI-567-greenfield-library-database-reset.md)
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-569-greenfield-media-delivery-service.md](./docs/pbis/active-alpha/PBI-569-greenfield-media-delivery-service.md)
- [PBI-570-greenfield-frontend-boundary-and-state-reset.md](./docs/pbis/active-alpha/PBI-570-greenfield-frontend-boundary-and-state-reset.md)
- [PBI-571-frontend-component-and-styling-consolidation-reset.md](./docs/pbis/active-alpha/PBI-571-frontend-component-and-styling-consolidation-reset.md)

Those PBIs define architecture. This PBI defines the words that architecture must use.

## Acceptance criteria
This PBI is complete only when:
- the reset PBIs all reference and follow this naming contract
- user-visible domain rows use `date_added`, `date_created`, and `date_modified` consistently
- public DTOs do not use weak names like `hash`, `kind`, `EntitySlim`, `imported_at`, or `path`
- backend and frontend shared API types use the same field names
- public contracts use semantic names instead of storage-history names

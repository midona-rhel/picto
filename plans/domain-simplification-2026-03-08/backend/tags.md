# Backend Tags

Read `../tags-system-truth.md` first. This file is only the backend slice.

Current footprint: about 4 files and about 1.7k lines

## What This Should Own

1. tag CRUD
2. aliases and parent relations
3. normalization
4. search and namespace summaries

## What This Should Not Own

1. selection UI rules
2. renderer formatting conventions
3. smart-folder policy beyond predicate evaluation support

## Why It Is Too Complicated

1. Tag normalization, relation logic, and query behavior are still close to other domains.
2. Tag changes fan out into grid, sidebar, smart folders, and selections, so invalidation is harder than it should be.
3. Frontend still compensates for backend shape in multiple places.
4. The backend currently mixes four different concerns under "tags":
   - generic tag normalization
   - external ingest coercion
   - relation graph management
   - site-specific metadata-to-tag mapping in subscriptions
5. The typed command layer, controller layer, and DB layer split responsibilities inconsistently.
6. The domain has both alias or sibling semantics and hard merge semantics, but the operational difference is not made explicit enough.

## Simplification Target

1. one normalized tag model
2. one relation service
3. one search/query surface
4. one explicit ingest policy boundary

## Concrete Work

1. Keep normalization entirely backend-side.
2. Make alias and parent operations share one relation policy surface.
3. Return already-normalized response shapes.
4. Move site-specific metadata tag extraction out of generic tag semantics and treat it as subscription ingest adapter behavior.
5. Decide and document the exact meaning of raw tag, display tag, sibling alias, parent, and merge.
6. Collapse the backend call path so commands go through one clear service surface instead of mixing controller and DB usage.

## Delete Or Merge

1. Merge relation helpers if they are split only by history.
2. Delete frontend-side tag normalization duplication.
3. Delete or fold thin controller methods that only pass through to DB calls.
4. Delete tag-maintenance actions triggered from normal UI entrypoints.

## Test Target

1. create or merge rename workflow
2. add and remove parent workflow
3. search and namespace summary workflow
4. external ingest workflow proving how unknown namespaces are treated
5. sibling alias versus merge workflow proving the difference explicitly

## Current Audit Findings

1. `normalize_ingested_namespaces` exists as a backend maintenance operation and is currently triggered from the frontend tag manager. Maintenance rewrites should not run because a screen opened.
2. `gallery_dl_runner.rs` contains large site-specific tag mapping logic. That may be necessary, but it is subscription ingest logic, not core tag-domain logic.
3. Batch tagging exists in both slow per-hash looping paths and optimized entity-id batch paths. The domain needs one obvious batch strategy.
4. Relation queries and mutation commands are serviceable, but the distinction between display aliases and canonical storage is still too implicit for a contributor to trust quickly.

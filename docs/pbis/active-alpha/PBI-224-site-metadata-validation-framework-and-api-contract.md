# PBI-224: Site metadata validation framework and API contract (global)

## Priority
P0

## Audit Status (2026-03-03)
Status: **Blocked (Subscription Workstream Deferred)**

Blocked Reason:
1. Subscription workstream is deferred by product direction for now.
2. Keep this PBI in backlog but do not execute until unblocked.

## Problem
Per-site metadata validation is currently distributed and partially implicit. We need one global contract and one validator pipeline that all site PBIs (198-223) plug into, otherwise behavior drifts and release quality is not enforceable.

## Scope
- `/Users/midona/Code/imaginator/core/src/gallery_dl_runner.rs`
- `/Users/midona/Code/imaginator/core/src/subscription_sync.rs`
- `/Users/midona/Code/imaginator/core/src/subscription_controller.rs`
- `/Users/midona/Code/imaginator/core/src/types.rs`
- `/Users/midona/Code/imaginator/src/types/api.ts`
- `/Users/midona/Code/imaginator/docs/pbis/PBI-198-site-metadata-validation-pixiv.md` ... `/Users/midona/Code/imaginator/docs/pbis/PBI-223-site-metadata-validation-instagram.md`

## API Contract (Global)
1. `subscriptions.get_site_metadata_schema(site_id)`
   - returns strict schema for required raw keys, required normalized fields, optional fields, and namespace mappings.
2. `subscriptions.validate_site_metadata({ site_id, sample_url, sample_metadata_json? })`
   - returns deterministic diagnostics for missing/invalid fields and normalized preview.
3. `subscriptions.validate_all_sites_metadata()`
   - CI-oriented command that validates fixtures for every registered site and fails on drift.
4. Sync event extensions
   - `subscription-progress`: include `metadata_validated`, `metadata_invalid`, `last_metadata_error`.
   - `subscription-finished`: include aggregate validation counters.

## Canonical Normalized Metadata Model
Required core fields:
- `site_id`
- `remote_post_id`
- `creator` (or explicit null with reason code)
- `source_urls[]`
- `tags[]`
- `validation_version`

Strongly-required when present in source:
- `title`
- `description`
- `post_published_at`
- `rating`
- `remote_creator_id`

## Implementation
1. Add a site schema registry keyed by `site_id`.
2. Run two-phase validation:
   - raw payload key/type validation,
   - normalized DTO validation.
3. Add reason codes for every validator failure path.
4. Add fixture packs and CI test command that covers all supported sites.
5. Block release on validation test failures.

## Acceptance Criteria
1. Every supported site has a concrete schema and fixture set.
2. Validation output is deterministic and typed for frontend consumption.
3. Subscription runs never ingest invalid metadata silently.
4. CI fails if any site schema/fixture validation regresses.

## Test Cases
1. Per-site valid fixture => validation pass.
2. Per-site missing required key fixture => expected deterministic failure.
3. Per-site type mismatch fixture => expected deterministic failure.
4. End-to-end subscription run emits non-zero validation counters only for invalid rows.

## Resume (Best Effort)
1. Add/maintain a site-specific resume strategy classification for this site (`supported` | `partial` | `unsupported`).
2. If supported, persist `subscription_query.resume_cursor` on interrupted runs and apply it on next run.
3. If unsupported, preserve explicit `unsupported` status in diagnostics and rely on archive-based de-duplication.
4. `Reset Subscription` must clear both query progress and this site's archive-prefixed entries for deterministic re-runs.

## Risk
High if skipped. This is required to make metadata quality contractual rather than best-effort.

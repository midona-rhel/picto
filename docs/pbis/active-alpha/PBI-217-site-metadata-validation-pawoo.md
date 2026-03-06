# PBI-217: Site metadata validation contract — Pawoo

## Priority
P0

## Audit Status (2026-03-03)
Status: **Blocked (Subscription Workstream Deferred)**

Blocked Reason:
1. Subscription workstream is deferred by product direction for now.
2. Keep this PBI in backlog but do not execute until unblocked.

## Problem
Metadata ingestion for `Pawoo` is not currently enforced with a strict, testable validation contract. For release, `Pawoo` must fail fast on invalid/missing metadata and expose deterministic API validation diagnostics before subscription runs.

## Site Capability Contract
- `site_id`: `pawoo`
- `domain`: `pawoo.net`
- `supports_query`: `no`
- `supports_account`: `yes`
- `auth_supported`: `yes`

## Scope
- `/Users/midona/Code/imaginator/core/src/gallery_dl_runner.rs`
- `/Users/midona/Code/imaginator/core/src/subscription_sync.rs`
- `/Users/midona/Code/imaginator/core/src/subscription_controller.rs`
- `/Users/midona/Code/imaginator/core/src/types.rs`
- `/Users/midona/Code/imaginator/src/types/api.ts`

## API Contract Additions (Required)
1. Add `subscriptions.validate_site_metadata` (dry-run validator):
   - input: `{ site_id, sample_url, sample_metadata_json? }`
   - output: `{ valid, missing_required_fields[], invalid_fields[], normalized_preview, warnings[] }`.
2. Add `subscriptions.get_site_metadata_schema`:
   - output includes required raw keys, required normalized fields, namespace mapping, and failure policy.
3. Extend sync progress/final events with validation counters:
   - `metadata_validated`, `metadata_invalid`, `last_metadata_error`.
4. Enforce ingestion gate:
   - invalid metadata rows are skipped with classified reason; never silently accepted.

## Required Raw Metadata Keys (Pawoo)
- `id` (status id)
- `account.acct`/author identity
- content text
- media_attachments
- created_at

## Required Normalized Metadata Fields (Pawoo)
- `remote_post_id`
- `creator`
- `description`
- `source_urls[]`
- `post_published_at`
- `tags[]` (hashtags)

## Implementation
1. Define `Pawoo` schema in a site-metadata schema registry (single source of truth).
2. Validate raw gallery-dl payload before normalize/ingest.
3. Normalize into canonical DTO and run post-normalization validation.
4. Reject invalid rows with explicit reason codes and counters.
5. Add fixture-driven tests for both valid and invalid `Pawoo` payloads.

## Acceptance Criteria
1. `Pawoo` payloads missing required keys fail validation deterministically.
2. `Pawoo` normalized metadata is complete and typed per schema.
3. Subscription runs surface metadata validation failures with actionable errors.
4. Tests cover success + failure cases with stable fixtures.

## Test Cases
1. Valid `Pawoo` payload -> `valid=true`, zero missing fields, normalized preview populated.
2. Missing critical keys -> `valid=false`, expected missing key list.
3. Type mismatch keys -> `valid=false`, expected invalid field diagnostics.
4. Live subscription sample -> validation counters emitted in progress/final events.

## Resume (Best Effort)
1. Add/maintain a site-specific resume strategy classification for this site (`supported` | `partial` | `unsupported`).
2. If supported, persist `subscription_query.resume_cursor` on interrupted runs and apply it on next run.
3. If unsupported, preserve explicit `unsupported` status in diagnostics and rely on archive-based de-duplication.
4. `Reset Subscription` must clear both query progress and this site's archive-prefixed entries for deterministic re-runs.

## Risk
Medium. External extractor payload drift requires robust schema versioning and fixture maintenance.

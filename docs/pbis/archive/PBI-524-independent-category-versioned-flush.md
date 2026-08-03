# PBI-524: Independent per-category versioned flush

## Priority
P2

## Problem
With the per-category bitmap split (status/tags/folders stored in separate files under `db/bitmaps/`), `flush_versioned()` still writes all three category snapshots on every publish cycle — even when only one category has dirty keys. In a typical workflow where a user is tagging files, only the `tags` category changes, but `status.v{N}.bin` and `folders.v{N}.bin` are also rewritten identically.

For large libraries (50k+ tags = ~150k bitmap keys), the tags category file can be several megabytes. Rewriting it is justified when tags are dirty. But rewriting the much smaller status and folders files alongside it is pure waste — and the real win of per-category storage is only realized when each category can version independently.

## Scope
- `core/src/sqlite/bitmaps.rs` — `flush_versioned()` only compacts dirty categories; clean categories keep their existing snapshot path
- `core/src/sqlite/publish.rs` — manifest tracks per-category version numbers (or a mapping of category → active file)
- `core/src/sqlite/open.rs` — `open_with_active_file()` parses per-category active files from the manifest payload

## Implementation
1. **Manifest payload change**: Replace the single `{"active_file":"bitmaps.v5.bin"}` payload with per-category tracking:
   ```json
   {
     "format": "per_category",
     "status": "status.v3.bin",
     "tags": "tags.v5.bin",
     "folders": "folders.v4.bin"
   }
   ```
   On load, if `format` key is missing, fall back to the current shared-version scheme for backward compat.

2. **`flush_versioned()` selective write**: Accept a version number, but only compact categories that are dirty. Return a struct (or JSON) describing which files were written, so the manifest can update only the changed entries.

3. **`open_with_active_file()` multi-file**: Parse the new payload format. For each category, load its specific versioned file. If the payload is in the old single-version format, derive the per-category paths from the shared version (backward compat).

4. **`prune_artifacts()` per-category**: Each category has its own version timeline. A status file is stale relative to the latest status version, not the latest tags version. Track keep-versions per category rather than a single shared keep set.

5. **Backward compatibility**: Libraries upgraded from the shared-version format will have all three categories at the same version. The first publish after upgrade will start diverging the version numbers naturally.

## Acceptance Criteria
1. Tagging a file only rewrites `tags.v{N}.bin` — `status` and `folders` files are untouched.
2. Changing file status only rewrites `status.v{N}.bin`.
3. Reopening a library after independent-version flush restores all bitmaps correctly.
4. Old manifest format (single `active_file`) still loads correctly (backward compat).
5. `prune_artifacts()` correctly prunes stale versions per-category without deleting current files from other categories.
6. `cargo test` passes with all existing and new bitmap tests.

## Test Cases
1. Dirty only tags → `flush_versioned()` writes new `tags.v{N}.bin` but `status` and `folders` snapshot paths unchanged.
2. Dirty all three → all three category files written with new version.
3. Reopen with mixed versions (status=v3, tags=v7, folders=v5) → all bitmaps intact.
4. Prune with mixed versions → only stale versions of each category removed.
5. Legacy payload `{"active_file":"bitmaps.v5.bin"}` → loads correctly, derives per-category paths.

## Risk
Low. The per-category file split is already done — this is a refinement of the versioning strategy. The main risk is backward-compat parsing of the manifest payload, which is mitigated by the format-detection fallback.

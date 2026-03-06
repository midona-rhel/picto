# PBI-238: Unify tag parsing paths into single canonical pipeline

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. Three separate tag parsing paths exist:
   - `tags.rs::parse_tag()` → full Hydrus-compatible cleaning (strip_tag_text_of_gumpf, colon disambiguation, leading-colon handling)
   - `sqlite/tags.rs::parse_tag_string()` → simplified split + lowercase, no cleaning pipeline
   - `tags.rs::parse_tag_ingest()` → ingest variant that rejects unknown namespaces
2. `dispatch/tags.rs` uses `parse_tag_string` for aliases/parents but `parse_tags` (via TagController) for add/remove.
3. The same tag string processed through different paths may produce different (namespace, subtag) pairs.
4. Colon escaping (`:has:colons` → empty namespace) is only consistent in `clean_tag`, not in `parse_tag_string`.

## Problem
Tag strings are parsed inconsistently depending on which code path they enter through. A tag entered via the alias system may be stored differently than the same tag entered via the add-tag system. This creates phantom duplicates, broken alias lookups, and makes it impossible to reason about tag identity without tracing the full call path.

## Scope
- `core/src/tags.rs` — `parse_tag`, `clean_tag`, `split_tag`, `parse_tag_ingest`
- `core/src/sqlite/tags.rs` — `parse_tag_string`
- `core/src/tag_controller.rs` — tag add/remove
- `core/src/dispatch/tags.rs` — alias/parent/merge command handlers

## Implementation
1. Define a single canonical tag normalization function that all paths must use. It should:
   - Lowercase
   - Strip garbage (control chars, zero-width joiners)
   - Handle colon disambiguation consistently
   - Validate namespace (reject emoticon prefixes)
   - Return `(namespace, subtag)` or error
2. Create two entry points that share the canonical core:
   - `normalize_tag(raw: &str) -> Result<(String, String), TagError>` — for user input (local tags)
   - `normalize_tag_ingest(raw: &str) -> Result<(String, String), TagError>` — for external sources (adds namespace allowlist filtering)
3. Delete `parse_tag_string` from `sqlite/tags.rs` — all callers use the canonical function.
4. Update all dispatch handlers to use the canonical entry points.
5. Add a migration or fixup command to normalize any tags in the database that were stored via the old inconsistent paths.

## Acceptance Criteria
1. Only one tag normalization function exists (with ingest variant).
2. `parse_tag_string` in `sqlite/tags.rs` is deleted.
3. All dispatch handlers (add, remove, alias, parent, merge) use the same normalization.
4. A tag entered via any path produces identical (namespace, subtag) storage.
5. Existing tags in the database can be normalized via a migration command.

## Test Cases
1. Tag `>:(` — parsed as unnamespaced emoticon through all paths.
2. Tag `:has:colons` — consistently parsed as (empty namespace, `has:colons`) through all paths.
3. Tag `artist:Bob` — consistently parsed as (`artist`, `bob`) through all paths.
4. Add tag via `add_tags`, then look up via `set_tag_alias` — same (ns, st) pair found.
5. Ingest tag `foo:bar` (unknown namespace) — treated as literal unnamespaced tag.

## Risk
Medium. Changing normalization may affect existing stored tags. The migration/fixup step is essential to avoid orphaned tag entries.

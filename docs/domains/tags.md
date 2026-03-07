# Tags Domain

## Purpose

Tags are the primary organizational primitive. Every file can have zero or more `(namespace, subtag)` pairs stored in the `entity_tag_raw` junction table. Tags support hierarchical organization via parent relationships, display aliasing via siblings, and search via FTS5.

## Tag Normalization Pipeline

All tag strings pass through a cleaning pipeline ported from Hydrus (HydrusTags.py):

1. **`clean_tag(raw)`** — lowercases, truncates to 1024 chars, handles leading-colon disambiguation, then calls `strip_tag_text_of_gumpf` on the whole tag and each component separately.
2. **`strip_tag_text_of_gumpf(t)`** — removes control characters, collapses whitespace, strips leading `- system:` garbage, removes Hangul filler chars (except in actual Hangul text), removes zero-width joiners from Latin-only text.
3. **`split_tag(cleaned)`** — splits on first `:` where the left side is a valid namespace (starts with a letter, contains only alphanumerics/underscores/hyphens/spaces). Rejects emoticon prefixes like `>:(`  by requiring alphabetic first char.
4. **`is_valid_namespace(s)`** — validates the namespace candidate.

### Two Canonical Entry Points

- **`parse_tag(raw) -> Option<(String, String)>`** — full `clean_tag` + `split_tag`. Used for all local operations (user input, DB lookups, search queries).
- **`parse_tag_ingest(raw) -> Option<(String, String)>`** — full `clean_tag` + `split_tag` + namespace allowlist check. Unknown namespaces are coerced to unnamespaced literals (`foo:bar` → `("", "foo:bar")`). Used for external sources (import, subscription, PTR feeds).

### Ingest Namespace Allowlist

13 namespaces accepted on ingest: `creator`, `studio`, `character`, `person`, `series`, `species`, `meta`, `system`, `artist`, `copyright`, `general`, `rating`, `source`. Any other namespace from an external source is treated as literal tag text.

## Lifecycle

1. **Import/Subscription** — tags arrive as raw strings, parsed via `parse_tag_ingest`, inserted via `add_tags_by_strings`.
2. **User tagging** — raw strings parsed via `parse_tag`, inserted via `add_tags_by_strings`.
3. **Storage** — `(namespace, subtag)` in `tag` table, membership in `entity_tag_raw`.
4. **Compiler** — `FileTagsChanged` event triggers bitmap rebuild for affected tags, rebuilds `Tagged` bitmap, updates smart folders.
5. **Display** — tag display uses sibling aliases: `display_ns` / `display_st` override storage values.

## Key Invariants

- Tag bitmaps (`BitmapKey::Tag`, `EffectiveTag`, `ImpliedTag`) must be updated whenever `entity_tag_raw` changes.
- The `Tagged` bitmap is the union of all files with at least one effective tag. Used to compute "untagged" view.
- Parent inheritance is resolved transitively: if A is parent of B, and B is parent of C, then files tagged C are also effectively tagged A and B.
- Sibling/alias relationships only affect display, not storage or bitmap membership.

## Gotchas

- Leading-colon disambiguation: a tag like `has:colons` is stored as `("", "has:colons")` and displayed as `:has:colons`. The leading colon in `clean_tag` is doubled to `::has:colons` format internally, then `split_tag` resolves it.
- The regex crate does not support look-ahead, so `is_leading_single_colon_no_more()` is a manual helper replacing the Python `^:(?=[^:]+$)` pattern.
- Hangul filler character (U+3164) is only stripped when the tag text does NOT contain actual Hangul characters — this preserves valid Hangul tags while removing invisible filler from Latin text.
- Zero-width joiners are stripped only from Latin-only text to preserve valid use in scripts like Devanagari.

## Key Files

- `core/src/tags.rs` — normalization pipeline, `parse_tag`, `parse_tag_ingest`
- `core/src/sqlite/tags.rs` — tag CRUD, FTS5 search, sibling/parent operations, batch tagging
- `core/src/tag_controller.rs` — orchestration layer for dispatch
- `core/src/dispatch/typed/tags.rs` — typed command handlers

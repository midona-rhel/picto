# Smart Folders Domain

## Purpose

Smart folders are saved predicate-based queries that automatically include files matching their rules. They are compiled to Roaring bitmaps for O(1) membership checks.

## Predicate Structure

```
SmartFolderPredicate
  └── groups: Vec<SmartRuleGroup>
        ├── match_mode: All | Any
        ├── negate: bool
        └── rules: Vec<PredicateRule>
              ├── field: "tags" | "rating" | "file_size" | "width" | ...
              ├── op: "include" | "gt" | "between" | "contains" | ...
              ├── value / value2 / values
```

## Compilation Logic

### Group Combination

Groups are ANDed together at the top level. This means a smart folder with multiple groups is an intersection — files must match ALL groups. Within each group, rules are combined according to the group's `match_mode`:
- `All` (AND): file must match every rule in the group
- `Any` (OR): file must match at least one rule

### Negation

If `group.negate` is set, the group result is inverted against `AllActive`:
```
result = AllActive - group_result
```

This means "files NOT matching this group's rules" — but only within active files (never includes trash).

### Rule Types

- **Tag rules**: look up tag_ids via batch query, then use `BitmapKey::EffectiveTag(tag_id)` bitmaps. Supports `include` (AND each), `include_any` (OR all), `do_not_include` (exclude).
- **Numeric rules**: SQL queries against file table columns (rating, size, width, height, duration, view_count, aspect_ratio). Supports eq/neq/gt/gte/lt/lte/between.
- **Text rules**: SQL LIKE queries against file table columns (notes, name, source_url). Supports is/is_not/contains/does_not_contain/starts_with/ends_with/is_empty/is_not_empty.
- **File type**: SQL against `mime` column.
- **Date**: SQL against `imported_at` column.
- **Shape**: SQL comparison of width vs height (landscape/portrait/square).
- **Color**: SQL against `file_color` table.
- **Has audio**: SQL against `has_audio` column.

### Result Scoping

All compiled results are intersected with `AllActive` — smart folders never include trash files.

## Compiler Integration

Smart folder bitmaps are cached in `BitmapKey::SmartFolder(id)`. The compiler rebuilds them when:
- `FileTagsChanged` — any tag change could affect tag-based predicates
- `FileInserted/Deleted/StatusChanged` — membership changes
- `TagGraphChanged` — parent changes affect effective tag membership
- `SmartFolderChanged` — predicate itself was modified

## Key Files

- `core/src/sqlite/smart_folders.rs` — CRUD, predicate types, `compile_predicate`, `compile_group`
- `core/src/smart_folder_controller.rs` — orchestration
- `core/src/dispatch/typed/smart_folders.rs` — typed command handlers

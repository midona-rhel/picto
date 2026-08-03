# PBI-520: Smart folder compiler selective rebuild

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-19). The performance concern is based on code inspection of the compiler event accumulation logic. The actual impact depends on how many smart folders a typical library has and how frequently tags change. For libraries with few smart folders, this optimization may provide negligible benefit. Profiling should validate the premise before implementation.

## Priority
P3

## Problem
When a `FileTagsChanged` event is received by the compiler system (`core/src/sqlite/compilers.rs`), it triggers `rebuild_all_smart_folders` — recompiling every smart folder's predicate into a bitmap, regardless of whether that smart folder references the changed tags.

For a library with many smart folders (e.g., 50+), this means every tag edit on a single file triggers 50+ bitmap set operations. The current approach is safe (correctness guaranteed) but potentially wasteful.

## Scope
- Analyze whether selective rebuild is feasible given the current smart folder predicate model
- If feasible: rebuild only smart folders whose predicates reference the changed tags
- If not feasible: document why and close this PBI

## Implementation
1. **Analyze predicate model**: Smart folders use `IncludeAll`, `IncludeAny`, `DoNotInclude` predicates over tag IDs. The predicate tags are known at compile time.
2. **Build dependency index**: When smart folders are compiled, build a reverse index: `tag_id -> Vec<smart_folder_id>` mapping which smart folders depend on which tags.
3. **On FileTagsChanged**: Look up which tags changed, consult the dependency index, and only rebuild the affected smart folders.
4. **Fallback**: If the dependency index is stale or the event doesn't include specific tag IDs, fall back to full rebuild.

## Acceptance Criteria
1. Tag edit on a file with 2 tags only rebuilds smart folders that reference those 2 tags (not all).
2. Smart folder contents are identical before and after the optimization (correctness preserved).
3. `cargo test` passes including existing compiler tests.
4. Performance improvement is measurable for libraries with 20+ smart folders.

## Test Cases
1. Library with 10 smart folders: edit a tag → only 2 smart folders (that reference that tag) are recompiled.
2. Library with 10 smart folders: delete a file → all smart folders recompiled (file deletion affects all scopes).
3. Tag graph change (parent/child edit) → all smart folders recompiled (correct fallback).

## Risk
Medium. The dependency index must be kept in sync with smart folder predicate changes. If the index becomes stale, incorrect smart folder contents could result. The full-rebuild fallback mitigates this but reduces the optimization's value.

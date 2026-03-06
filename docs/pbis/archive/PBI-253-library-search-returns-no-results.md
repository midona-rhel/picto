# PBI-253: Library search field returns no results

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. A user on Linux reported: "Search field in the library view returns nothing."
2. The user had imported 50 images from a local folder (likely without tags/metadata from a booru source).
3. Possible causes:
   - Search relies on FTS5 index which may not be populated for locally imported files without tags.
   - Search may only search tags, not filenames or other metadata.
   - FTS5 index may not have been rebuilt after import.
   - Could be a UI issue where results are returned but not displayed.

## Problem
The search field in the library view does not return results for a user with 50 imported images. This could be a genuine bug (search broken) or a discoverability issue (search only works on tags, not filenames, and local imports may not have tags).

## Scope
- Search input handler and query dispatching
- Backend search command — what fields does it search (tags only? filenames? notes?)
- FTS5 index state — is it populated correctly after local imports?
- Search UX — does the search field indicate what it searches?

## Implementation
1. **Investigate**: determine what the search field actually queries (tags, filenames, notes, or all).
2. If search only queries tags: locally imported files with no tags will never appear. Either:
   - Expand search to include filenames and other metadata (preferred)
   - Or clearly indicate in the search placeholder: "Search tags..." so users understand the scope
3. Verify FTS5 index is populated after local import (not just subscription import).
4. If this is a bug (FTS5 not populated or search query malformed), fix the root cause.
5. Add placeholder text to the search field indicating what it searches.

## Acceptance Criteria
1. Search returns results for locally imported files (at minimum by filename).
2. Search placeholder text indicates what is searchable.
3. FTS5 index is populated correctly after all import paths (local, subscription, drag-and-drop).
4. Empty search results show a helpful message (e.g. "No results — try searching by tag or filename").

## Test Cases
1. Import 50 local images → search by a filename substring → results found.
2. Import tagged images from Danbooru → search by tag → results found.
3. Search with a query matching nothing → "No results" message shown.
4. Search immediately after import (no restart) → results available.

## Risk
Medium. If search is currently tag-only by design, expanding it to filenames requires changes to the FTS5 schema and query builder. If it's a bug, the fix may be simpler.

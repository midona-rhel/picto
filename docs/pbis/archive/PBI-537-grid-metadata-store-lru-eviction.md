# PBI-537: Grid metadata store LRU eviction

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The eviction strategy concern is based on code inspection of `gridMetadataStore.ts`. If the 5000-entry cache is large enough that eviction rarely occurs in practice, this optimization has negligible impact. The `metadataPrefetch.ts` layer already uses LRU — the concern is about the store-level cache specifically.

## Priority
P3

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: `src/state/gridMetadataStore.ts` uses a `Map<string, FileMetadataSlim>` as a metadata cache with a maximum size of 5000 entries. When the cache exceeds this limit, entries are evicted in FIFO order (insertion order, which `Map` preserves). The `metadataPrefetch.ts` layer uses a proper byte-budgeted LRU cache, but the store-level cache does not.

## Problem
The grid metadata store evicts entries in FIFO (first-in, first-out) order when the cache exceeds 5000 entries. This means the first items cached (typically from the grid page the user viewed first) are evicted first — even if the user scrolled back to them recently.

In a typical browsing pattern (scroll down, scroll back up, navigate to folder, go back), FIFO evicts items the user is likely to revisit. LRU (least recently used) eviction would keep recently-viewed items in cache regardless of when they were first loaded.

## Scope
- `src/state/gridMetadataStore.ts` — metadata cache eviction logic

## Implementation
1. **Promote on access**: When a cache hit occurs in `metadataCache.get(hash)`, move the entry to the end of the Map (delete + re-insert). This converts the Map from FIFO to LRU ordering.
   ```typescript
   getMetadata(hash: string): FileMetadataSlim | undefined {
     const entry = this.metadataCache.get(hash);
     if (entry) {
       // Promote to most-recently-used
       this.metadataCache.delete(hash);
       this.metadataCache.set(hash, entry);
     }
     return entry;
   }
   ```
2. **Eviction on insert**: When inserting and cache exceeds limit, delete the first entry (least recently used, since promotions move accessed items to the end).
3. **Alternative — use an existing LRU library**: If a dedicated LRU implementation exists in the codebase (check `metadataPrefetch.ts`), reuse its data structure.

## Acceptance Criteria
1. Recently accessed metadata entries are retained longer than unaccessed entries.
2. Cache size stays bounded at 5000 entries.
3. No visual or behavioral regression in the grid.
4. Scroll-back-and-forth pattern has higher cache hit rate than before.

## Test Cases
1. Fill cache to 5000 entries → access entry #1 → insert entry #5001 → entry #1 is still in cache (LRU promoted), entry #2 is evicted.
2. Fill cache to 5000 entries → don't access entry #1 → insert entry #5001 → entry #1 is evicted (FIFO within LRU).
3. Grid scroll: load page 1 (100 items) → scroll to page 50 → scroll back to page 1 → all page 1 items are cache hits.

## Risk
Low. The change is isolated to the metadata cache in a single store. The delete-then-reinsert pattern for Map promotion is a well-known JavaScript idiom. The main risk is performance — Map delete+set is O(1) amortized, but doing it on every cache read adds a small constant cost.

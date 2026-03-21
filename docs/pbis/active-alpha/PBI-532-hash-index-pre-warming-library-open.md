# PBI-532: Hash index pre-warming on library open

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The cold-start latency was identified by code inspection — the HashIndex LRU starts empty on every library open. Actual user-perceived latency depends on how quickly the first grid page loads and whether the N DB lookups for cache misses are noticeable. Profiling the first grid load should validate the premise.

## Priority
P2

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: `HashIndex` in `core/src/sqlite/resolve.rs` is an LRU cache (50,000 entries) mapping `SHA256 hash → file_id`. It starts empty when a library is opened. The first grid page load triggers N cache misses (one per displayed item), each requiring an individual DB round-trip. For the default page size of 100 items, this means 100 individual SELECTs on the first load.

## Problem
On library open, the hash index LRU cache is empty. The first grid page load incurs N cache misses (one per displayed item), each requiring an individual DB round-trip. For the default page size of 100 items, this means 100 individual SELECTs on the first load.

Subsequent page loads are fast (cache hits), but the cold-start adds perceptible latency to the first interaction after opening a library.

## Scope
- `core/src/sqlite/resolve.rs` — HashIndex struct and cache population
- `core/src/sqlite/open.rs` or `core/src/state.rs` — library open sequence

## Implementation
1. **Add `warm_cache()` method** to `HashIndex`:
   ```rust
   pub async fn warm_cache(&self, db: &SqliteDatabase) -> Result<(), String> {
       // Load the N most recently imported files (matches default grid sort)
       let pairs = db.with_read_conn(|conn| {
           let mut stmt = conn.prepare(
               "SELECT hash, file_id FROM file WHERE status = 1
                ORDER BY imported_at DESC LIMIT ?"
           )?;
           let rows = stmt.query_map([self.capacity()], |row| {
               Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
           })?;
           rows.collect::<rusqlite::Result<Vec<_>>>()
       }).await?;
       self.insert_batch(pairs);
       Ok(())
   }
   ```
2. **Call during library open**: After `SqliteDatabase::open()` succeeds, call `db.hash_index.warm_cache(&db)` before returning the state.
3. **Batch insert method**: Add `insert_batch(pairs: Vec<(String, i64)>)` to HashIndex to populate the LRU in bulk without N individual lock acquisitions.

## Acceptance Criteria
1. After library open, the first grid page load has zero hash cache misses (for recently imported files).
2. Library open time does not increase by more than 50ms.
3. Cache warm uses a single SQL query, not N individual lookups.
4. Libraries with fewer files than the cache capacity warm all entries.

## Test Cases
1. Library with 1000 files → open → first grid page (100 items, sorted by imported_at DESC) → all cache hits.
2. Library with 100,000 files → open → cache warms 50,000 most recent → first page all hits.
3. Empty library → open → warm is a no-op (no crash, no error).
4. Library open + warm completes in under 200ms for a 50k-file library.

## Risk
Low. The warm query is a simple SELECT with LIMIT, hitting the indexed `status + imported_at` composite index. The only risk is if the warm query blocks library open for too long on very large libraries — the LIMIT clause bounds this.

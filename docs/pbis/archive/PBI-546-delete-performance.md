# PBI-546: Batch delete performance

## Priority
P2

## Problem
Batch deleting many files is extremely slow. The current implementation likely processes each file individually (blob delete, DB delete, bitmap update, event emission). For thousands of files this takes minutes.

Also: deleting should be a hard delete (remove from DB + blob store), distinct from trashing (status change). The current behavior may be conflating the two.

## Implementation
- Batch all DB deletes into a single transaction
- Batch blob store deletes (parallel I/O)
- Single bitmap rebuild after the batch, not per-file
- Single mutation event after the batch
- Ensure delete != trash — delete removes data, trash changes status

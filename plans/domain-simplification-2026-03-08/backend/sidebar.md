# Backend Sidebar

Current footprint: about 3 files and about 405 lines

## What This Should Own

1. sidebar tree query shape
2. aggregate counts needed by the sidebar

## What This Should Not Own

1. folder business logic
2. smart-folder business logic
3. subscription business logic

## Why It Is Too Complicated

1. Sidebar is a read-model, not a domain with rich business behavior.
2. It becomes confusing when folder or smart-folder policy leaks into sidebar code.
3. The sidebar should aggregate other domains, not become one.

## Simplification Target

1. sidebar as read-model assembly only
2. counts derived from canonical backend facts

## Concrete Work

1. Keep tree assembly logic in one place.
2. Ensure counts are derived, not independently maintained.
3. Move non-read-model behavior back to the owning domain.

## Delete Or Merge

1. Delete sidebar helpers that mutate domain data.
2. Merge count calculation with runtime receipt generation if it reduces duplication.

## Test Target

1. sidebar tree composition workflow
2. counts refresh workflow after status change
3. tree update workflow after folder or smart-folder CRUD

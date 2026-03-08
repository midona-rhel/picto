# Backend Folders

Current footprint: about 4 files and about 2.5k lines

## What This Should Own

1. folder CRUD
2. parent-child hierarchy
3. folder membership
4. folder reorder and sorting semantics

## What This Should Not Own

1. sidebar composition policy
2. grid rendering concerns
3. collection behavior that is really its own thing

## Why It Is Too Complicated

1. Folder behavior leaks into sidebar, selection, and collection logic.
2. Reorder, membership, and hierarchy semantics are not isolated cleanly.
3. Folder and collection behavior are close enough to drift, but not formalized enough to share correctly.

## Simplification Target

1. one folder service for hierarchy and membership
2. one explicit place for reorder semantics
3. collection-specific behavior moved out if it is not actually folder behavior

## Concrete Work

1. Split hierarchy operations from membership operations.
2. Keep reorder math in one place.
3. Make folder events use one mutation pattern.

## Delete Or Merge

1. Merge reorder helpers into the folder service instead of scattering them.
2. Delete duplicated membership update paths.

## Test Target

1. create, move, and delete folder workflow
2. add and remove file membership workflow
3. reorder items and reorder folders workflow

# Backend Scope

Current footprint: about 2 files and about 333 lines

## What This Should Own

1. canonical scope resolution rules
2. folder, smart-folder, status, and tag scope composition

## What This Should Not Own

1. grid-specific pagination
2. selection-specific bulk mutation logic

## Why It Is Too Complicated

1. Scope is cross-cutting but currently easy to duplicate.
2. If grid and selection resolve scope differently, the entire product feels unreliable.

## Simplification Target

1. one canonical scope resolver used by all read-side and selection flows

## Concrete Work

1. Pull scope construction out of any domain that is re-implementing it.
2. Make all major query domains depend on this one contract.

## Delete Or Merge

1. Delete duplicate scope logic in grid and selection helpers.

## Test Target

1. exact-same-result tests for grid and selection scope resolution

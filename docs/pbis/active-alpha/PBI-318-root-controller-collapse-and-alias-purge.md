# PBI-318: Root controller collapse and alias purge

## Priority
P1

## Problem
The backend root still contains multiple domain controller homes that should disappear once domain folderization lands.

## Scope
- Root controller files such as:
  - `folder_controller.rs`
  - `tag_controller.rs`
  - `smart_folder_controller.rs`
  - `selection_controller.rs`
  - `metadata_controller.rs`
- legacy re-exports and alias paths in `lib.rs`

## Implementation
1. Move controller behavior into `domains/*` canonical homes.
2. Remove root-level compatibility homes once imports migrate.
3. Reduce `lib.rs` to top-level topology exports only.
4. Delete alias paths that would let old imports linger.

## Acceptance Criteria
1. Root controller files in scope are gone.
2. Domain behavior is owned by `domains/*` modules.
3. `lib.rs` no longer exposes flat-root legacy topology.
4. Net backend LOC decreases materially.

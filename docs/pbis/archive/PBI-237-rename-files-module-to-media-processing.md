# PBI-237: Rename core files/ module to media_processing/

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. `core/src/files/` contains: MIME detection, thumbnail generation, hashing, archive extraction, ffmpeg integration, SVG rendering, PDF parsing, office document handling, color extraction, blurhash.
2. The name `files` suggests it owns the file domain (CRUD, status, metadata), but it's actually a stateless media processing toolkit.
3. The `dispatch/files.rs` router and `sqlite/files.rs` persistence layer also exist, creating three unrelated `files` modules at different levels.

## Problem
Three things are named `files`:
- `core/src/files/` — media processing utilities (MIME, thumbnails, hashing)
- `core/src/dispatch/files.rs` — file domain command routing
- `core/src/sqlite/files.rs` — file table persistence

This naming collision makes it ambiguous what "files" means in any given context and which module owns what responsibility.

## Scope
- `core/src/files/` → rename to `core/src/media_processing/` (or `media/`, `file_processing/`)
- All `crate::files::` imports across the crate

## Implementation
1. Rename `core/src/files/` to `core/src/media_processing/`.
2. Update `lib.rs` module declaration.
3. Find-and-replace all `crate::files::` to `crate::media_processing::`.
4. Verify build and tests pass.

## Acceptance Criteria
1. `core/src/files/` no longer exists as a directory.
2. `core/src/media_processing/` contains all former files/ contents.
3. `cargo build` and `cargo test` pass.
4. No ambiguity between the processing module, dispatch routing, and persistence.

## Test Cases
1. `cargo build` — compiles cleanly.
2. `cargo test` — all tests pass.
3. Thumbnail generation, MIME detection, hashing all work.

## Risk
Low. Mechanical rename with no logic change. Single PR.

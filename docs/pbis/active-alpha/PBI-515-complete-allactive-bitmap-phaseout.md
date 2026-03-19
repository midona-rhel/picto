# PBI-515: Complete AllActive bitmap phaseout

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-19). The findings are based on static code inspection and may not reflect actual runtime impact or project priorities. Human review is recommended before acting on it.

## Priority
P1

## Problem
The `BitmapKey::AllActive` variant was documented as deprecated ("backward-compat alias") but is still actively used in 6 files across 11 call sites. The tags compiler (`tags/compiler.rs:29`) explicitly copies `Status(1)` into `AllActive` on every compile cycle, creating a redundant bitmap that is then read by the sidebar compiler and folder service.

This means every bitmap flush writes two bitmaps for the same set of "active" files, and every compile cycle does an unnecessary copy. The code comment says the key is kept "to avoid rewriting the bitmap snapshot format", but the key could simply be removed from the enum and the format version bumped to trigger a rebuild on next library open.

## Evidence
| File | Line(s) | Usage |
|------|---------|-------|
| `core/src/sqlite/bitmaps.rs` | 30-35, 590, 634 | Enum definition, serialization, deserialization |
| `core/src/sqlite/compilers.rs` | 499, 603, 652, 671 | Test assertions checking AllActive |
| `core/src/folders/service.rs` | 52 | `let active_bm = db.bitmaps.get(&BitmapKey::AllActive)` for sidebar count |
| `core/src/sidebar/compiler.rs` | 100 | `let active_bitmap = bitmaps.get(&BitmapKey::AllActive)` |
| `core/src/tags/compiler.rs` | 29 | `bitmaps.set(BitmapKey::AllActive, bitmaps.get(&BitmapKey::Status(1)))` — pure redundancy |
| `core/src/tags/compiler.rs` | 213 | `tagged &= &bitmaps.get(&BitmapKey::AllActive)` |
| `core/src/tags/db.rs` | 894 | `let all_active = self.bitmaps.get(&BitmapKey::AllActive)` |

## Scope
- Remove the `AllActive` variant from `BitmapKey` enum
- Replace all `AllActive` reads with `Status(1)` reads
- Remove the redundant `set(AllActive, ...)` call in tags compiler
- Update bitmap serialization format version to trigger rebuild on next open
- Update all test assertions

## Implementation
1. In `core/src/sqlite/bitmaps.rs`: Remove `AllActive` from the enum. Update `to_bytes()`/`from_bytes()` to handle the removed variant gracefully (skip on read, never write).
2. In `core/src/tags/compiler.rs`: Delete line 29 (`bitmaps.set(BitmapKey::AllActive, ...)`). Replace line 213 `AllActive` with `Status(1)`.
3. In `core/src/folders/service.rs`: Replace line 52 `AllActive` with `Status(1)`.
4. In `core/src/sidebar/compiler.rs`: Replace line 100 `AllActive` with `Status(1)`.
5. In `core/src/tags/db.rs`: Replace line 894 `AllActive` with `Status(1)`.
6. In `core/src/sqlite/compilers.rs`: Update test assertions from `AllActive` to `Status(1)`.
7. Bump bitmap format version constant so existing libraries rebuild bitmaps on next open.

## Acceptance Criteria
1. `grep -r "AllActive" core/src/` returns zero matches.
2. `cargo test --manifest-path core/Cargo.toml` passes.
3. `cargo check --manifest-path core/Cargo.toml` passes with no warnings about the removed variant.
4. Existing libraries open correctly and rebuild bitmaps on first launch after the change.

## Test Cases
1. Open an existing library after the change — bitmaps rebuild without error.
2. Create a new library — no AllActive bitmap appears in bitmaps.bin.
3. Sidebar counts match before/after the change for a library with known file counts.

## Risk
Low. The `AllActive` bitmap is already semantically identical to `Status(1)` — the tags compiler ensures this. Replacing reads is a mechanical substitution with no behavioral change.

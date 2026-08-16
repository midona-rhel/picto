# Duplicates & Merge Behavior

## Overview

Duplicate detection uses perceptual hashing (`img_hash` DoubleGradient 16x16, 256-bit). Candidate pairs are
stored in the `duplicate` table for manual review or automatic resolution. Similarity is computed
from physical files; review decisions address the owning media entities.

## Resolution Actions

| Action | Behavior |
|--------|----------|
| `smart_merge` | Picks higher-quality file as winner, merges metadata, deletes loser |
| `keep_left` | Left file wins, merges metadata from right, deletes right |
| `keep_right` | Right file wins, merges metadata from left, deletes left |
| `not_duplicate` | Marks pair as false positive, no files deleted |
| `keep_both` | Dismisses pair, no files deleted |

## What Happens to the Loser

### Media entity

The intended result is that the loser's `media_entity`, file record, and unreferenced blob are
removed. Before deletion:

- Tags, source URLs, notes are merged onto the winner
- Rating is consolidated (higher value wins)
- Timestamps are consolidated (earliest `imported_at` and `created_at` are kept)
- Folder memberships and subscription associations transfer to the winner

One physical file has one image or video media entity, so duplicate resolution never keeps a second
entity that points at the winner's file. Metadata and external references move to the winner, and the
loser entity/file are deleted. Source-post metadata and folder memberships remain explicit properties
of the surviving entity; duplicate resolution never creates or chooses a hidden group.

Blob reclamation and reference repointing are release evidence, not assumptions. A duplicate
resolution is not certified until the smoke/test path proves both the database state and physical
original/thumbnail cleanup.

### Edge cases

- **Source-post metadata**: Preserved on the surviving media entity

## Auto-Merge During Subscription Import

When `auto_merge_enabled` is true, newly imported images are checked against existing files through
the ingest duplicate path. Exact perceptual hash matches may auto-resolve only for comparable
static images when the quality decision is unambiguous. Near matches are imported and create
review work; they are not silently rejected or merged.

`All` is the accepted active library. Inbox and Trash are separate lifecycle scopes and
must not inflate `All`, folder, smart-folder, or main-search counts. Duplicate review deliberately
spans Active and Inbox so newly imported media can be reviewed before acceptance; Trash is excluded
from duplicate candidates and duplicate-related sidebar counts.

## Key Files

| File | Role |
|------|------|
| `core/src/db/mod.rs` | Native duplicate scan, candidate pagination, and resolution entrypoints |
| `core/src/db/query/duplicates.rs` | File-based scan sources, candidate reads, and duplicate counts |
| `core/src/db/write/duplicates.rs` | Candidate persistence and resolution/repointing writes |
| `core/src/duplicates/quality.rs` | Conservative quality comparison for merge decisions |
| `core/src/dispatch/typed/duplicates.rs` | IPC handlers for scan, review, and resolution |
| `core/src/engine/duplicates.rs` | Application-engine duplicate boundary |
| `core/src/blob_store.rs` | Physical original/thumbnail blob storage and reclamation |

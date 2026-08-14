# PBI-602: Folder-based library sync

## Problem

Picto has local operation-log and filesystem-sync foundations, but folder sync is not yet a
complete user behavior. A second Picto installation must be able to consume a library folder
transported by Google Drive, Dropbox, or equivalent desktop software without sharing a live
SQLite database.

## Behavior

- The user selects a sync folder. Picto does not sign in to a cloud provider.
- Picto writes immutable media blobs and versioned metadata operations into that folder.
- The provider's desktop application transports those files between devices.
- Each device keeps its own SQLite database, queues, credentials, thumbnails, and run history.
- Subscription groups, subscription definitions, and query configuration sync; credentials,
  cursors, counters, errors, jobs, and run history remain device-local.
- One `sync_cycle` uploads local work, consumes remote work in order, verifies hashes, and rebuilds
  affected projections.
- Missing prerequisites remain pending and retry later. Unknown operation versions stop sync with a
  clear update-required error.
- Sync never deletes or overwrites a local library because a remote file is malformed or missing.

## Acceptance

- Startup, periodic, and manual sync call the same `sync_cycle`.
- Media blobs are available and hash-verified before metadata that references them settles.
- Paths and symlinks cannot escape the selected sync folder.
- Interrupted upload/download resumes without duplicate operations or skipped work.
- Two devices converge after offline edits and restart.
- Credentials, subscription run history, thumbnails, queues, and cached projections never sync.
- The UI reports last success, active work, pending prerequisites, and failures truthfully.
- A packaged two-device folder-sync smoke passes before this PBI is removed.

## Verification

Completed 2026-08-14:

- `cargo test --manifest-path core/Cargo.toml 'oplog::' -- --nocapture`: 84 passed.
- The packaged macOS smoke launched Picto as device A, device B, then restarted device A using
  separate installation homes and local databases over one temporary filesystem share.
- A media import and folder created on A reached B; B verified the original blob's SHA-256, renamed
  the folder, and restarted A received that rename.
- Device identities were distinct, A retained its identity after restart, final sync state had no
  pending prerequisites or missing/failed blobs, and the temporary share was removed.
- Cold-start Library Manager discovery listed the installed Google Drive and iCloud providers with
  no open library and no library-scoped backend error.

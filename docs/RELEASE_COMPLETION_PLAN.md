# Picto Release Completion Plan

## Goal

Finish Picto around one understandable backend: SQLite truth, rebuildable bitmap projections, one
application operation path, one compact invalidation contract, and one durable subscription worker.
Collections are first-class library items. Cloud Sync and Tutorials are implemented release
surfaces covered by the final smoke. No deferred or unimplemented PBI blocks this release.

## Release Rules

1. Visible library items are standalone media or collections.
2. Media assets use the accepted-format registry. Collection roots own lifecycle and folders;
   members inherit. ZIP imports are safely expanded into one collection.
3. `All` means active accepted roots only. Inbox and Trash stay outside All and library scopes.
4. The durable ingest queue is the only entrypoint for manual, watch, subscription, and retry work.
5. Before 1.0, there are no migrations. A library must match the current schema or fail untouched.
6. Every mutation settles SQLite and projections before emitting one revision/resource invalidation.
7. PBIs close only after focused tests and an application-level smoke; completed PBIs are deleted.
8. The pre-release macOS alpha uses Picto's private self-signed certificate and is intentionally not
   notarized. Public Apple Developer ID signing and notarization are separate release work.

## Phase 1: Backend Replacement

- Replace the layered backend with direct application operations and a current exact schema.
- Model library roots, media assets, physical files, collection membership, folders, tags, smart
  folders, source provenance, ingest work, and subscriptions explicitly.
- Make collection lifecycle and folder membership authoritative on the root; members inherit them.
- Implement group, detach, ungroup, reorder, cover selection, collection metadata fan-out, and
  destructive collection deletion.
- Use one canonical item query for grid, outline, selection, export, details, and counts.
- Keep projections incremental and rebuildable; never rebuild the whole library for ordinary writes.

## Phase 2: Communication Cutover

- Replace detailed state-change payloads with `revision`, affected resource keys, and item IDs.
- Resources are `library`, `sidebar`, `folders`, `smart_folders`, `tags`, `duplicates`,
  `subscriptions`, `settings`, `tasks`, and `item:{id}`.
- Frontend consumers re-query canonical data after committed invalidation. No speculative grid
  insertion or renderer-owned count patches remain.
- Verify drag-to-Inbox, drag-to-All, drag-to-Trash, folder moves, imports, and deletion settle in
  every open view without stale or ghost items.

## Phase 3: Subscriptions and Ingest

- Use one durable worker for scheduled and manual subscription runs, retry, stop, and restart.
- Persist source posts, source items, downloads, ingest state, retries, and terminal outcomes.
- Resume from non-terminal work; use source identity for idempotency and preserve deletion tombstones.
- Stream source items through the durable ingest queue. A multi-file post promotes its first item
  into a collection when the second item arrives, then appends later items in source order.
- Normalize all adapters to one post/item contract, sanitize descriptions centrally, and retain
  direct-site login through the OS credential store.
- Certify the supported source registry only after login, metadata, pagination, restart, and
  terminal-state behavior pass.

## Phase 4: Implemented Product Surface

- Verify duplicates with deterministic quality comparison, metadata/provenance preservation, and
  safe collection-aware merge behavior.
- Verify tag management and durable automatic AI tagging.
- Keep Picto's accepted-format registry aligned with the product's documented format matrix. Accepted
  files are preserved even when Picto has no detail renderer yet. The current matrix is:
  - Image/texture: `bmp`, `gif`, `heic`, `heif`, `hif`, `icns`, `ico`, `jpeg`, `jpg`, `jpe`,
    `jfif`, `png`, `svg`, `tif`, `tiff`, `webp`, `avif`, `base64`, `insp`, `jxl`, `dds`, `exr`,
    `hdr`, `tga`, `svga`, `pag`, `lottie`, `iff`, `lbm`, plus existing Picto image formats.
  - 3D: `fbx`, `obj`, `3ds`, `3mf`, `dae`, `ifc`, `ply`, `stl`, `glb`, `gltf`, `vrm`.
  - Design source: `af`, `afdesign`, `afphoto`, `afpub`, `ai`, `c4d`, `cdr`, `clip`, `dwg`,
    `graffle`, `idml`, `indd`, `indt`, `mindnode`, `psb`, `psd`, `psdt`, `pxd`, `principle`,
    `sketch`, `skt`, `skp`, `xd`, `xmind`.
  - Video: `m4v`, `mp4`, `webm`, `mov`, `mkv`, `flv`, `f4v`, `ts`, `mts`, `m2ts`, `3gp`,
    plus existing Picto video formats.
  - Audio: `aac`, `flac`, `m4a`, `mp3`, `ogg`, `wav`, plus existing Picto audio formats.
  - Font/RAW: `ttf`, `ttc`, `otf`, `woff`, `3fr`, `arw`, `cr2`, `cr3`, `crw`, `dng`, `erf`,
    `mrw`, `nef`, `nrw`, `orf`, `pef`, `raf`, `raw`, `rw2`, `sr2`, `srw`, `x3f`.
  - Office/web: `txt`, `md`, `markdown`, `json`, `key`, `numbers`, `pages`, `pdf`, `potx`,
    `ppt`, `pptx`, `xls`, `xlsx`, `doc`, `docx`, `eddx`, `emmx`, `html`, `mhtml`, `url`.
  - Explicit extension additions: `zip` (expand to collection), `epub`, `swf`, `eps`, `livp`,
    LUT (`cube`, `3dl`), and lighting photometry (`ies`).
- Treat external format research as a post-release behavior backlog, not code or assets to copy.
  It does not block this release. Candidate
  Picto-native actions: custom export; Live Photo, SWF, JXL, EPS, EPUB, and video-format support;
  FFmpeg/media-info dependencies; UTF-8 repair; video-frame export; combine images; OCR/copy text;
  crop; video-to-GIF; image comparison; EXIF; histogram; format conversion; reverse/high-resolution/
  Pinterest image search; AI models/actions/search/enlarge/background removal/erase; and MCP server.
  Each accepted item needs a separate behavior decision before implementation.
- Verify deletion, recently viewed, folder/smart-folder behavior, and representative performance.
- Keep OnlyFans as a separate source runner using the same normalized subscription contract.
- Verify Cloud Sync through committed application operations and revision events, including its
  persistence and packaged application behavior.
- Verify Tutorials against stable navigation/command identifiers without duplicating product
  surfaces or using synthetic host input.

## Phase 5: Release Gate and Cleanup

- Delete replaced engine, DB façade, compiler/change-impact, renderer patch, and duplicate paths.
- Delete tests that only prove mocked forwarding; retain behavior and persistence tests.
- Remove commands, dependencies, documentation, and PBIs without active callers or release value.
- Run Rust formatting/tests, TypeScript/Vitest, command parity, native build, packaged Electron
  smoke, fresh-library schema checks, restart recovery, and representative performance probes.
- Delete this replacement PBI and other completed PBIs only after their focused smoke passes.

## Acceptance

- One production path exists for each user mutation, ingest path, query model, invalidation model,
  and subscription worker.
- Collections, All/Inbox/Trash, folders, smart folders, sidebar counts, grid counts, tags,
  duplicates, subscriptions, and restart recovery agree on persisted state.
- Cloud Sync and Tutorials pass the packaged smoke without bypassing application operations or
  duplicating product UI.
- Deferred optional PBIs are not release gates.
- No pre-1.0 migration code exists.

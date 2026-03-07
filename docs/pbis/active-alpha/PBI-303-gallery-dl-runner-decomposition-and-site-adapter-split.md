# PBI-303: Gallery-dl runner decomposition and site adapter split

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `core/src/gallery_dl_runner.rs` is 2400+ lines.
2. It currently owns site registry data, query URL templating, credential shaping, temp config generation, subprocess spawning, output scanning, sidecar parsing, and failure interpretation.
3. Site-specific behavior is encoded in one giant runner module rather than in explicit adapters.
4. Auth requirements and query capabilities are represented as static data inside the same module that spawns child processes.

## Problem
`gallery_dl_runner.rs` is a monolith. Registry, capability description, authentication shaping, process execution, and result parsing are all coupled together. This makes adding or debugging a source unnecessarily risky and keeps subscription behavior tied to one oversized file.

## Scope
- `core/src/gallery_dl_runner.rs`
- supporting credential and query-shaping helpers

## Implementation
1. Split the runner into:
   - site registry/catalog
   - site capability/auth metadata
   - query/url builder
   - process runner
   - output parser / metadata ingestion reader
   - failure classifier
2. Introduce explicit per-site adapter configuration rather than one giant static registry file.
3. Keep the process runner generic and free of site-specific branching.
4. Move credential translation into dedicated helpers.

## Acceptance Criteria
1. `gallery_dl_runner.rs` no longer contains all source registry and execution logic in one file.
2. Site capability metadata is isolated from process management.
3. A new source can be added by editing a focused adapter/registry area rather than the whole runner.
4. Failure handling is testable independently of download execution.

## Test Cases
1. Known working sources still run successfully after the split.
2. Auth-required source configuration still maps credentials correctly.
3. Failure kinds remain correctly classified.

## Risk
Medium-high. Broad refactor of the downloader execution layer.

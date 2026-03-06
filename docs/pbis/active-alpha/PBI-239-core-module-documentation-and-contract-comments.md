# PBI-239: Core module documentation and contract comments

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. Module-level docs are one-line summaries: `//! Tag CRUD, file tagging, search (FTS5), sibling/parent operations.`
2. No function-level documentation explaining contracts, invariants, or *why* complex logic exists.
3. The tag cleaning pipeline (`strip_tag_text_of_gumpf`, colon disambiguation, leading-colon rules) has no explanation of why these rules exist or what breaks if they change.
4. `MutationImpact` has no documentation on when to use which domains or which convenience constructors to prefer.
5. The dispatch routing order (files → grid → tags → folders → ...) has no documented significance.
6. For an AI-assisted workflow, missing documentation means every code change requires re-deriving intent from implementation.

## Problem
The codebase has almost no documentation beyond trivial one-liners. Complex logic (tag cleaning rules, bitmap invalidation, dispatch routing, MutationImpact construction) is undocumented. This forces every contributor (human or AI) to reverse-engineer intent from implementation, leading to incorrect fixes and accumulating drift between intended and actual behavior.

## Scope
- All `core/src/` modules — add module-level architectural docs
- Key functions in `tags.rs`, `events.rs`, `dispatch/mod.rs`, `dispatch/common.rs` — add contract docs
- `sqlite/tags.rs`, `sqlite/bitmaps.rs`, `sqlite/compilers.rs` — add invariant docs

## Implementation

### Per-domain markdown files (primary deliverable)

Each domain gets its own markdown doc in `docs/domains/` (or `core/docs/`). One file per domain, covering:

- **Purpose**: What this domain is responsible for.
- **Lifecycle**: How data flows in and out (commands, events, persistence).
- **Key invariants**: What must always be true (e.g. "tag bitmaps must be updated whenever entity_tag_raw changes").
- **Contracts**: What each public function promises (parameters, return values, side effects, error conditions).
- **Gotchas**: Non-obvious behavior, edge cases, known workarounds.

Example structure:
```
docs/domains/
  tags.md          — tag parsing, storage, search, siblings, parents, ingest rules
  dispatch.md      — routing model, command naming, argument conventions
  events.md        — MutationImpact rules, domain invalidation, event lifecycle
  folders.md       — folder CRUD, sidebar projection, entity membership
  subscriptions.md — gallery-dl integration, flow lifecycle, credential handling
  ptr.md           — PTR sync, bootstrap, delta updates
  sqlite.md        — connection pool, WAL, bitmaps, compilers, schema migrations
  ...
```

### Code-level docs (secondary)

1. **Module-level docs**: Each Rust module gets a 5-10 line `//!` doc comment summarizing purpose, ownership, and key invariants.
2. **Why-comments**: Complex logic (tag cleaning regex, bitmap key schemes, dispatch routing order, MutationImpact domain rules) gets comments explaining *why*, not what.
3. Prioritize the most-touched and most-confusing modules first: dispatch, events, tags, sqlite.

### Ordering

Write the markdown domain docs first — they're the most useful for AI-assisted workflows and new contributors. Code-level doc comments second, as ongoing work.

## Acceptance Criteria
1. Every domain has a dedicated markdown file in `docs/domains/`.
2. Each domain doc covers purpose, lifecycle, invariants, contracts, and gotchas.
3. Every module in `core/src/` has a module-level doc comment of 3+ lines.
4. Complex logic has why-comments explaining the reasoning.
5. A new contributor (or AI) can understand how a domain works by reading its markdown file without diving into implementation.

## Test Cases
1. `cargo doc --no-deps` generates meaningful documentation for all public items.
2. Code review: a reviewer unfamiliar with the codebase can understand module responsibilities from domain docs alone.
3. AI agent given a domain doc can make correct modifications without reverse-engineering intent.

## Risk
Low. Pure documentation — no code behavior changes. Can be done incrementally alongside other work.

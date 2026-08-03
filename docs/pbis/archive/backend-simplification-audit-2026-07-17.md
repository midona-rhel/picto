# Backend Simplification Audit — working checklist

Goal: walk the entire backend, area by area, and confirm each piece **makes sense**
(clear single responsibility, right layer) and is **not over-complicated** (no needless
indirection, duplication, dead surface, or accidental complexity). Reduce code where safe.

Hard rules: no feature removal · no behavior change without proof · every step ends green
(`cargo check` + `cargo test --lib` + `npx tsc --noEmit` + `node scripts/check-command-parity.mjs`
+ app boots). The `subscriptions/` + `ingest*` tree is deferred until the other session lands
its refactor.

Audit questions applied to every area:
1. Responsibility — can you state what this module does in one sentence? Does anything in it belong elsewhere?
2. Layering — does it re-implement something dispatch/engine/db already owns? Is any layer a pure pass-through that could be thinner?
3. Surface — which `pub` items have zero or one caller? Which commands/flags/params are never exercised from the frontend, scripts, or tests?
4. Complexity smells — needless `async`, `Arc`/`Mutex` without sharing, stringly-typed errors hiding real types, clones on hot paths, double serialization, config that is never varied.
5. Tests — do tests live next to what they test, and do risky paths have any?

---

## Status legend
`[x]` done · `[~]` in progress · `[ ]` pending · `[D]` deferred (other session's lane)

## Steps

### 0. [x] Baseline + quick wins (done this session)
- Deleted orphaned `core/src/sqlite/` (−2,917 lines, never compiled).
- `db/mod.rs` 4,730 → 2,786: tests → `db/tests.rs`; collection/tag/folder/smart-folder/deferred-work ops → sibling files.
- `from_args` dispatch helper (14 sites), `SidebarNodePatch: Default` (10 literals),
  `engine.rebuild_sidebar()` (11 sites), runtime_contract import aliases (3 files).
- Resolved 6 stash-pop conflicts in frontend controllers + re-deleted resurrected ghost file.

### 1. [x] Command surface audit (dispatch + parity allowlist)
- Verdicts: 10 of 59 rust-only entries were **scanner false-positives** (sort keys / status
  strings from typed handlers' unrelated matches) — scanner now restricts match-arm extraction
  to mod.rs; allowlist cleaned to 49. Parity + scanner unit tests pass.
- Of the 49 real commands: 2 have external callers (`resolve_file_paths_batch` → electron IPC,
  `ensure_thumbnail` → media protocol). The other 47 are the **legacy/dormant surface** for
  features not yet rebuilt in the new frontend (tag manager, duplicates review, export,
  find_similar, companion_*) — kept per no-feature-removal rule, now documented in the
  allowlist comment. Follow-up (needs sign-off): retire pairs provably superseded by
  engine-routed commands (e.g. `add_tags`/`remove_tags` vs `apply_entity_tags`).

### 2. [x] `runtime_contract/` (change_builder 522 + state_change)
- Verdict: coherent design, but **over-specified relative to consumption**. 18 of 52 builder
  methods have zero core callers; 8 StateChange fields are emitted but never read by the
  frontend runtime (`member_hashes`, `tag_changes`, `tag_structure_changed`,
  `media_fields_changed`, `group_ids`, `subscription_ids`, `query_ids`,
  `credential_categories`) — the frontend invalidates at coarser granularity (Domain level).
- **No cuts now**: several "unused" methods (`batch_tags`, `tags_added`) appear in the other
  session's stashed work; the granular fields are their likely wiring targets. Revisit after
  their refactor lands — if still unread then, trim both sides of the contract together.

### 3. [x] `engine/` layer sense-check
- Verdict: **makes sense as-is**. The engine is a uniform façade: ~27 read methods are pure
  `self.db.x()` pass-throughs, writes add the transaction boundary + `commit_write`/events.
  The pass-through ceremony is the price of a single routing surface for dispatch — grouping
  or macro-ing them would save lines but cost grep-ability. No change.
- Deferred actionable: 33 typed handlers return `serde_json::Value` (double serialization) —
  convert to concrete return types case by case, test-backed, separate change.

### 4. [~] `db/` remainder
- `db/mod.rs` (2,786): still holds open/init + kv/oplog + entity/file ops + bulk + reads. Split entity/file ops and reads once the other session's ingest work lands (they touch these).
- [x] `apply_remote_op`: verdict — the flat 21-op match is catalog-style and readable; kept flat,
  relocated with its helpers to `db/remote_ops.rs` (−463 lines from mod.rs → **2,324**).
- [x] `db/query/grid.rs` + `db/projection/smart_folders.rs`: audited — both internally
  well-factored (scope/filter/sort helpers; per-rule compile fns + tests). Sound, no change.
- `migration_legacy/` (521): correctness-critical, audit read-only — flag, don't touch.

### 5. [x] Media pipeline family — see above.

### 6. [x] Infrastructure singletons
- Verdicts: `start_workers` 8-arg signature has ONE call site that predates AppState —
  params struct would be churn, kept. All Arc/Mutex wrapping justified (shared across
  worker tasks). events/oplog/rate_limiter/settings/credential_store: sound.
- Done: `perf.rs` trimmed to the one live metric (sidebar_tree) — removed 5 dead
  recorders and fixed the SLO that could never pass (it gated on metrics nothing
  records); `poison.rs` dead read/write_or_recover removed.

### 7. [x] Small domain modules
- folders/smart_folders/duplicates/selection/import/ai_tagger: sound, no dead surface.
- Done: deleted `scope/resolver.rs` (369 lines — abandoned alternative scope
  implementation, zero references); removed 6 dead `tags/db.rs` fns (two were
  non-collection-aware duplicates of the live batch path) + `tags/logging.rs`
  preview helper (141 lines total).
- Deferred (medium risk, test-backed): folder-rank SQL helper extraction in
  `dispatch/typed/folders.rs`.

### 8. [x] Electron host
- Done: `windowManager.mjs` 1,201 → **505** — the ~640-line embedded-auth machinery
  (cookie-login capture, booru API-key scraping, Pixiv OAuth popup) moved to
  `windows/authSessions.mjs` behind a `createAuthSessions({BrowserWindow, getMainWindow})`
  factory; windowManager façade and IPC surface unchanged; app boots.
  `registerHandlers.mjs` (486): skimmed — uniform thin handlers, sound.

### 9. [D] `ingest.rs` + `ingest_queue.rs` (1.7k each)
- Deferred: other session actively refactoring (currently mid-flight, import briefly broken).
- When free: the two files overlap in naming — confirm the queue/executor split is real.

### 10. [D] `subscriptions/` tree (~8k lines)
- Deferred: other session's lane. Largest audit surface in the backend when it frees up
  (runtime_service 1,621 + credential_service 1,161 + runtime_db 1,006 + sync_engine…).

### 11. [ ] Cross-cutting finish pass
- Error strategy: `Result<_, String>` everywhere — acceptable for IPC boundary, but check internal layers aren't stringifying real error types early.
- `#[allow(dead_code)]` / `#[doc(hidden)]` inventory.
- Re-run: full `alpha:verify` lane as the final gate.

---

## Log
- 2026-07-17: Step 0 completed. Steps 1-8 audited/executed same day: ~3,600 lines of
  dead code removed repo-wide (sqlite/ 2,917 + scope/resolver 369 + perf ~100 +
  media_derivatives 57 + tags/poison 141 + wrappers ~40), db/mod.rs 4,730 -> 2,324,
  windowManager 1,201 -> 505. All gates green after every change (cargo test 153/153,
  tsc, parity, app boots). Remaining: steps 9-11 + deferred medium-risk items.

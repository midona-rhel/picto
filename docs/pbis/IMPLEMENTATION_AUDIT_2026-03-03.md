# Alpha Blocker Status (Reset Baseline)

Date: 2026-03-04

This file replaces the prior broad audit snapshot with an alpha-release-focused blocker matrix.

## Alpha Blockers

| Blocker | Owner | Target Date | Required Evidence |
| --- | --- | --- | --- |
| Fix TypeScript compile contract mismatch in `ImageGrid`/`CanvasGrid` | Frontend Runtime | March 6, 2026 | `npx tsc -p tsconfig.json --noEmit` passes in CI (`alpha:verify`) |
| Restore frontend test stability (`imageGridSizeGuard` and related regressions) | Frontend Runtime | March 7, 2026 | `npm run test -- --run` passes in CI (`alpha:verify`) |
| Validate inbox/subscription live updates and sort integrity | Grid + Subscription Owners | March 8, 2026 | Passing `eventBridge.inboxSubscriptionImport` + sidebar drag/drop tests, plus alpha smoke reports |
| Validate sidebar/grid count consistency for status mutations | Core Dataflow | March 8, 2026 | Passing status-mutation tests and `alpha:smoke` report for status move scenario |
| Cross-platform alpha packaging (macOS/Windows/Linux) | Release Engineering | March 10, 2026 | `alpha:package` matrix green and package artifacts uploaded |
| Cross-platform smoke execution reports recorded | Release Engineering | March 10, 2026 | `alpha-smoke-<platform>` artifacts uploaded for all 3 platforms |
| PBI index and active backlog cleanup complete | Product + Release | March 6, 2026 | `docs/pbis/README.md` reflects active/archived split and links resolve |

## Active Alpha Backlog Source of Truth

Use `docs/pbis/active-alpha/` as the only active PBI set for alpha execution.

## Non-Blocking Debt

Architecture/style budget checks moved to the non-blocking legacy lane:

1. `guard:no-raw-colors`
2. `guard:grid-architecture`
3. `check:file-sizes`
4. `check:undo-coverage`
5. any additional `guard:all` checks not required by `alpha:verify`

See `docs/release/LEGACY_CHECKS.md`.

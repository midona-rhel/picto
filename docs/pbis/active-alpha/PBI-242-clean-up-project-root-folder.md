# PBI-242: Clean up project root folder

## Priority
P2

## Audit Status (2026-03-07)
Status: **Implemented**

Resolution:
1. `AUDIT.md` moved to `docs/AUDIT.md`.
2. HTML entry points (`detail.html`, `settings.html`, `subscriptions.html`, `library-manager.html`, `index.html`) remain at project root — this is a Vite multi-page app constraint. Documented in `vite.config.ts` lines 20-22 above the `rollupOptions.input` block.
3. Root now contains only: config files, package management, HTML entry points (documented), and source directories. No stale artifacts remain.

Previous evidence (2026-03-06):
1. The project root contained audit files (`AUDIT.md`) mixed with config and entry points.
2. HTML entry points for Electron windows were at root without documentation of why.
3. `AUDIT.md` was a one-off artifact that belonged in `docs/`.
4. The root was cluttered enough that navigating it required scrolling past unrelated files.

## Problem
The project root is a dumping ground. HTML entry points for different Electron windows, audit documents, config files, and build tooling all sit at the same level. This makes it hard to scan the root and understand the project structure at a glance.

## Current root contents and proposed locations

| File | Current | Proposed |
|---|---|---|
| `AUDIT.md` | root | `docs/AUDIT.md` |
| `detail.html` | root | `src/windows/detail/index.html` or keep at root (Vite requirement) |
| `settings.html` | root | `src/windows/settings/index.html` or keep at root |
| `subscriptions.html` | root | `src/windows/subscriptions/index.html` or keep at root |
| `library-manager.html` | root | `src/windows/library-manager/index.html` or keep at root |
| `index.html` | root | stays (Vite main entry) |
| `Cargo.toml`, `Cargo.lock` | root | stays (workspace root) |
| `package.json`, `yarn.lock` | root | stays |
| `tsconfig.json`, `tsconfig.node.json` | root | stays |
| `vite.config.ts`, `vitest.config.ts` | root | stays |
| `postcss.config.cjs` | root | stays |

## Scope
- Move audit/doc artifacts to `docs/`
- Evaluate whether HTML entry points can be moved into `src/` (depends on Vite multi-page config)
- Clean up any other stale root-level files

## Implementation
1. Move `AUDIT.md` to `docs/`.
2. Investigate Vite multi-page app configuration — HTML entry points may need to stay at root or can be moved with `rollupOptions.input` configuration in `vite.config.ts`.
3. If HTML files can be moved, co-locate each with its source entry point (e.g. `detail.html` near `src/detail.tsx`).
4. If HTML files must stay at root (Vite constraint), document why in a comment in `vite.config.ts`.
5. Review for any other stale files at root level.

## Acceptance Criteria
1. No audit/documentation artifacts at root level — moved to `docs/`.
2. HTML entry points are either moved or explicitly documented as a Vite requirement.
3. Root folder is clean enough to understand the project structure at a glance.

## Test Cases
1. `npm run dev` — all Electron windows still load correctly.
2. `npm run build` — production build succeeds.
3. All HTML entry points resolve correctly after any moves.

## Risk
Low-medium. HTML entry point moves depend on Vite multi-page configuration. If Vite requires root-level HTML, the files stay but get documented.

# Subscriptions Domain

## Purpose

Subscriptions automate file acquisition from external sources (websites, galleries) via gallery-dl. A subscription is a saved query (URL pattern) that periodically checks for new content and imports it.

## Entity Hierarchy

- **Flow** — groups multiple subscriptions with a shared schedule. Execution runs all subscriptions in the flow sequentially.
- **Subscription** — a named source with one or more queries.
- **Query** — a specific URL or search pattern within a subscription. Tracks cursor position for incremental fetching.

## Sync Engine Lifecycle

1. **Trigger** — manual run or scheduled flow execution.
2. **gallery-dl subprocess** — runs gallery-dl with the query URL, outputs JSON metadata per downloaded file.
3. **Metadata validation** — gallery-dl output is parsed, validated, and mapped to import parameters.
4. **File import** — each downloaded file is imported through the standard import pipeline with tags from `parse_tag_ingest`.
5. **Cursor update** — query cursor is advanced so the next run only fetches new content.
6. **Events** — `subscription-started`, `subscription-progress`, `subscription-finished` events are emitted throughout.

## Credential Handling

Site credentials are stored in the OS keychain via `credential_store.rs`. Credentials are injected into gallery-dl's config at runtime. The credential domain in `sqlite/subscriptions.rs` stores which sites have saved credentials.

## Key Files

- `core/src/subscription_sync.rs` — sync engine, gallery-dl orchestration
- `core/src/subscription_controller.rs` — subscription/flow CRUD orchestration
- `core/src/flow_controller.rs` — flow CRUD, execution scheduling
- `core/src/gallery_dl_runner.rs` — gallery-dl subprocess management
- `core/src/credential_store.rs` — OS keychain integration
- `core/src/sqlite/subscriptions.rs` — subscription, query, credential CRUD
- `core/src/sqlite/flows.rs` — flow CRUD
- `core/src/dispatch/typed/subscriptions.rs` — typed command handlers

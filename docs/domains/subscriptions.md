# Subscriptions Domain

A subscription is a named, scheduled set of source queries. Users run the subscription as a unit;
queries retain their own source cursor and provenance.

Subscriptions are top-level. There is no group entity or group-level run state.

## Run Lifecycle

1. A manual or scheduled trigger creates one durable run.
2. Query jobs execute sequentially through the source adapter and gallery-dl runner, except dedicated
   runners such as OnlyFans.
3. Each discovered file is queued immediately through the canonical media ingest path.
4. Download, queue, import, skip, and failure progress is persisted and reported from durable state.
5. A query cursor advances only after its durable work is accounted for.
6. The run succeeds, stops, or remains retryable with a truthful persisted reason.

Restart resumes durable jobs without losing prior run totals or re-importing deliberately deleted
content. One failed query must not falsify another query's result.

## Source Metadata

Each imported entity retains source ID, post ID, item key, page order, canonical post URL, media URL,
source tags, and supported source-specific fields. Adapters normalize source output; they may not
invent tag namespaces or bypass the canonical ingest path.

## Authentication

Every source has one direct-site login flow. Picto opens the real source page in a managed browser,
captures the resulting session, and stores it in the operating-system credential store. The product
does not ask users to paste passwords, cookies, tokens, or API keys.

## Certification

A source is visible only after the strict production-path harness proves discovery, download,
independent-media ingest, metadata, cursor resume, restart, replay, and the real Electron workflow.

## Ownership

- `core/src/subscriptions/runtime_service.rs`: public subscription behavior
- `core/src/subscriptions/run_orchestrator.rs`: durable run coordination
- `core/src/subscriptions/job_queue.rs`: query job claims and retries
- `core/src/subscriptions/gallery_dl_runner.rs`: gallery-dl process ownership
- `core/src/subscriptions/source_adapter/`: source normalization
- `core/src/subscriptions/sync_engine/`: download-to-ingest handoff and persistence
- `core/src/subscriptions/credential_service.rs`: direct-site session handling
- `core/src/subscriptions/runtime_db.rs`: durable subscription runtime state

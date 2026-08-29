# Greenfield Backend Benchmark Ledger

Run in release mode on the local ARM64 macOS development host:

```sh
cargo run --release -p picto_library --example scale_probe -- 100000
```

The fixture has 100,000 roots, 20 tags per root, 10,000 distinct tags, active/Inbox/Trash
lifecycle distribution, MIME and palette facts, URLs, and fixed-rate concurrent page reads during
ingestion. FTS and derivative settlement are intentionally outside canonical ingest timing.

| Measurement | Existing backend baseline | Greenfield result |
|---|---:|---:|---:|
| Concurrent reader p95 | 4.69 ms | 1.877 ms |
| Concurrent reader p99 | Not recorded | 2.838 ms |
| Concurrent reader maximum | Not recorded | 5.101 ms |
| Warm page read p95 | Not recorded | 0.371 ms |
| Sidebar counts, 100k roots | Not recorded | 4.592 ms |
| Selection summary, 94k active roots | 36 ms at 90k | 4.743 ms |
| Add tag to 94k active roots | 42 ms at 90k | 5.292 ms |
| Move 94k active roots to Trash | Not recorded | 0.892 ms |
| Canonical ingest database work | About 4 ms/item | 0.240 ms/item |
| Ingest publication batch p95 | Not recorded | 13.550 ms |
| Publication gate p95 | Not recorded | 4.096 ms |
| Publication gate maximum | Not recorded | 5.678 ms |
| Estimated projection memory | Not recorded | 17.684 MiB |
| Projection checkpoint write | Not recorded | 64.088 ms |
| Checkpoint-backed reopen | Not recorded | 37.374 ms |
| Full projection recovery | Not recorded | 255.884 ms |
| Projection checkpoint size | Not recorded | 12,214,412 bytes |

This run uses the measured 48-item canonical publication cap. Selection summary timing includes six
ordered preview hashes, shared metadata, image-root capability, and collection action candidates.

The numbers are a development ledger, not a portable hardware guarantee. Release acceptance still
requires the complete mutation matrix, smart-folder rebuilds, FTS freshness, crash injection,
projection memory measurement, one-million-root fixtures, and packaged platform smoke tests.

## 300k Settlement Scaling

The same fixture at 300,000 roots verifies that canonical ingestion no longer rescans every
persisted bitmap shard or copies complete per-root projection maps on every publication.

| Measurement | Before targeted projection work | Current result |
|---|---:|---:|
| First 100k ingest interval | 24,824.769 ms | 19,929.768 ms |
| Second 100k ingest interval | 53,422.801 ms | 23,934.057 ms |
| Third 100k ingest interval | 74,604.288 ms | 28,770.039 ms |
| Total canonical ingest | 152,851.893 ms | 72,633.916 ms |
| Database work per item | 0.510 ms | 0.242 ms |
| Publication batch p95 | 32.605 ms | 12.771 ms |
| Concurrent reader p95 | 3.858 ms | 2.222 ms |
| Concurrent reader p99 | 5.823 ms | 3.346 ms |
| Concurrent reader maximum | 32.892 ms | 9.873 ms |
| Projection memory estimate | 37.173 MiB | 47.103 MiB |
| Checkpoint-backed normal reopen | 99.038 ms | 94.486 ms |

The projection memory increase is the measured cost of small copy-on-write owner and palette
shards. It remains well below the 512 MiB one-million-root target when extrapolated linearly.
Checkpoint-backed reopen is normal startup; a full bitmap reconstruction is a recovery path only.

## Post-Cutover Regression Check

The current release build was rerun after the canonical notes, exact-hash ingestion, duplicate,
and automatic AI-tagging work. The 100,000-root comparison remains within normal run-to-run noise
of the recorded greenfield result. Automatic AI work is not enabled by this canonical-ingest probe.

| Measurement | Recorded greenfield | Before optimization | Current 100k |
|---|---:|---:|
| Canonical ingest database work | 0.240 ms/item | 0.249 ms/item | 0.131 ms/item |
| Ingest publication batch p95 | 13.550 ms | 14.122 ms | 7.853 ms |
| Concurrent reader p95 | 1.877 ms | 1.887 ms | 2.162 ms |
| Concurrent reader p99 | 2.838 ms | 2.916 ms | 2.787 ms |
| Warm page read p95 | 0.371 ms | 0.323 ms | 0.258 ms |
| Selection summary | 4.743 ms | 4.979 ms | 4.582 ms |
| Sidebar counts | 4.592 ms | 4.757 ms | 4.086 ms |
| Add tag to 94k active roots | 5.292 ms | 5.667 ms | 4.971 ms |
| Publication gate p95 | 4.096 ms | 4.096 ms | 4.096 ms |
| Projection memory | 17.684 MiB | 17.684 MiB | 17.684 MiB |

A symbolized process trace found repeated SQLite parsing in canonical bitmap persistence and the
per-item identity/insert path. Reusing cached statements and allocating IDs with one
`UPDATE ... RETURNING` statement removed that parser work. No schema, transaction boundary, or
durability behavior changed.

## One-Million-Root Probe

The current release build completed the same fixture at one million roots and twenty million tag
assignments. A separate macOS storage-analysis process consumed approximately one CPU core during
this run, so CPU-bound timings are conservative. Reads remained responsive throughout ingestion,
and projection memory remained below its target. The stricter summary, count, ingest-publication,
and publication-gate budgets did not pass at this scale.

| Measurement | Before optimization | Current 1M | Target | Result |
|---|---:|---:|---:|---|
| Canonical ingest database work | 0.468 ms/item | 0.319 ms/item | Below 5 ms/item | Pass |
| Ingest publication batch p95 | 32.550 ms | 25.041 ms | Below 16 ms | Miss |
| Concurrent reader p95 | 4.753 ms | 5.696 ms | Below 50 ms | Pass |
| Concurrent reader p99 | 7.456 ms | 8.606 ms | Below 100 ms | Pass |
| Concurrent reader maximum | 40.725 ms | 31.177 ms | Below 500 ms | Pass |
| Warm page read p95 | 0.744 ms | 0.500 ms | Below 50 ms | Pass |
| Selection summary, 940k active roots | 46.146 ms | 41.559 ms | Below 20 ms | Miss |
| Sidebar counts | 29.877 ms | 25.547 ms | Below 20 ms | Miss |
| Add tag to 940k active roots | 53.694 ms | 48.685 ms | Below 10 s at 1M | Pass |
| Move 940k active roots to Trash | 21.525 ms | 20.642 ms | Below 5 s at 1M | Pass |
| Publication gate p95 | 16.384 ms | 16.384 ms | Below 5 ms | Miss |
| Publication gate maximum | 42.689 ms | 32.451 ms | No reader stall above 500 ms | Pass |
| Projection memory | 161.763 MiB | 161.763 MiB | Below 512 MiB | Pass |
| Checkpoint write | 2,674.351 ms | 2,562.022 ms | Recovery/maintenance path | Recorded |
| Checkpoint-backed reopen | 304.485 ms | 304.649 ms | Normal startup path | Recorded |
| Checkpoint size | 117,444,458 bytes | 117,444,458 bytes | No fixed budget | Recorded |

The optimized canonical ingest intervals were 52.147 seconds for roots 1-300k, 90.247 seconds for
300-600k, 132.274 seconds for 600-900k, and 44.772 seconds for 900k-1M. Total time fell from
468.327 to 319.441 seconds. The remaining growth comes from serializing and writing denser touched
tag-bitmap shards, not reader blocking: fixed-rate reader latency stayed well inside the
responsiveness budget for the entire run.

## One-Million-Root Smart-Folder Hierarchy

The release probe uses 1,000 smart folders across eight levels with a
`20/380/400/120/50/20/8/2` tapered distribution. Every folder contains a distinct dense ten-rule
structured filter, allowing up to 80 inherited rules at depth eight. Each local filter owns a
precomputed bitmap; inheritance is one parent-result/local-result intersection. Independent broad
local-result updates run on a bounded worker pool that leaves one logical core available to visible
work.

| Measurement | Repeated ancestor evaluation | Precomputed local, serial | Current unique-filter result |
|---|---:|---:|---:|
| Create all 1,000 definitions | 10,406.250 ms | 5,096.963 ms | 4,895.318 ms |
| Depth-eight edit p95, one result | 13.035 ms | 7.248 ms | 6.708 ms |
| Depth-three edit p95, six results | 55.960 ms | 7.381 ms | 6.808 ms |
| Depth-one edit p95, fifty-two results | 415.819 ms | 8.792 ms | 7.608 ms |
| All roots cease matching all 1,000 folders | Not recorded | 80.166 ms | 40.747 ms |
| All roots positively settle into all 1,000 folders | 2,968.023 ms | 3,033.216 ms | 251.302 ms |
| Projection memory estimate | Not recorded | 606.107 MiB | 363.293 MiB |

The positive full settlement is the bounded adversarial case: one million affected roots and all
1,000 distinct filters depending on the changed field. Ordinary ingestion settles only newly
affected root bits. Shared effective results and direct broad replacement keep the projection below
the 512 MiB target. Text-dependent settlement remains on the serialized low-priority FTS path.

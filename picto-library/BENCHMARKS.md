# Greenfield Backend Benchmark Ledger

Run in release mode on the local ARM64 macOS development host:

```sh
cargo run --release -p picto_library --example scale_probe -- 100000
```

The fixture has 100,000 roots, 20 tags per root, 10,000 distinct tags, active/Inbox/Trash
lifecycle distribution, MIME and palette facts, URLs, and fixed-rate concurrent page reads during
ingestion. FTS and derivative settlement are intentionally outside canonical ingest timing.

| Measurement | Existing backend baseline | Greenfield result |
|---|---:|---:|
| Concurrent reader p95 | 4.69 ms | 1.633 ms |
| Concurrent reader p99 | Not recorded | 2.419 ms |
| Concurrent reader maximum | Not recorded | 8.302 ms |
| Warm page read p95 | Not recorded | 0.284 ms |
| Sidebar counts, 100k roots | Not recorded | 3.393 ms |
| Selection summary, 94k active roots | 36 ms at 90k | 4.755 ms |
| Add tag to 94k active roots | 42 ms at 90k | 4.915 ms |
| Move 94k active roots to Trash | Not recorded | 0.961 ms |
| Canonical ingest database work | About 4 ms/item | 0.192 ms/item |
| Ingest publication batch p95 | Not recorded | 14.053 ms |
| Estimated projection memory | Not recorded | 16.700 MiB |
| Projection checkpoint write | Not recorded | 53.374 ms |
| Checkpoint-backed reopen | Not recorded | 33.124 ms |
| Full projection recovery | Not recorded | 255.884 ms |
| Projection checkpoint size | Not recorded | 12,214,412 bytes |

This run uses the final 64-item canonical publication cap. Selection summary timing includes six
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

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
| Concurrent reader p95 | 4.69 ms | 1.767 ms |
| Concurrent reader p99 | Not recorded | 2.495 ms |
| Concurrent reader maximum | Not recorded | 5.962 ms |
| Warm page read p95 | Not recorded | 0.194 ms |
| Sidebar counts, 100k roots | Not recorded | 2.095 ms |
| Selection summary, 94k active roots | 36 ms at 90k | 0.271 ms |
| Add tag to 94k active roots | 42 ms at 90k | 6.225 ms |
| Move 94k active roots to Trash | Not recorded | 0.802 ms |
| Canonical ingest database work | About 4 ms/item | 0.247 ms/item |
| Ingest publication batch p95 | Not recorded | 13.944 ms |
| Estimated projection memory | Not recorded | 13.599 MiB |
| Projection checkpoint write | Not recorded | 58.714 ms |
| Checkpoint-backed reopen | Not recorded | 32.532 ms |
| Full projection recovery | Not recorded | 255.884 ms |
| Projection checkpoint size | Not recorded | 12,346,910 bytes |

The numbers are a development ledger, not a portable hardware guarantee. Release acceptance still
requires the complete mutation matrix, smart-folder rebuilds, FTS freshness, crash injection,
projection memory measurement, one-million-root fixtures, and packaged platform smoke tests.

# PBI-620: Certify Coomer And Kemono CDN Publication

## Observed Gap

Pawchive, Coomer, and Kemono are native providers with fixture coverage. Pawchive has passed the
durable live certification. Coomer and Kemono metadata APIs and thumbnail edges were reachable on
2026-08-30, but their original-file paths redirect to or explicitly name the shared `n1`-`n3`
storage shards. Connections from the certification host to those shard addresses timed out over
both IPv4 and IPv6 before TLS, so the production-path runs could not receive original media bytes
within the bounded 120-second certification window. Picto must not substitute the reachable
`img.*/thumbnail/data/*` previews for original files. Both runs cancelled cleanly without
publishing false success, returned their query to pending, and produced no certification report or
root.

## Required Behavior

- Repeat the Coomer and Kemono public live certification from a host that can retrieve original
  bytes from their `n1`-`n3` storage shards.
- Do not add a provider-specific scheduler, downloader, timeout bypass, or authentication surface to
  compensate for an external CDN outage.

## Acceptance

1. A Coomer live public smoke proves serial settlement, restart, continuation, canonical tags,
   stored blobs, and pacing.
2. A Kemono live public smoke proves the same behavior.

# PBI-620: Restore Missing Subscription Providers

## Observed Gap

Pawchive, Coomer, and Kemono are native providers with fixture coverage. Pawchive has passed the
durable live certification. Coomer and Kemono metadata endpoints were reachable on 2026-08-30, but
their redirected media CDN endpoints did not return bytes within 20 seconds from the certification
host, so complete live publication is not yet proven.

## Required Behavior

- Repeat the Coomer and Kemono public live certification from a host that can retrieve their media
  CDN bytes.
- Do not add a provider-specific scheduler, downloader, timeout bypass, or authentication surface to
  compensate for an external CDN outage.

## Acceptance

1. A Coomer live public smoke proves serial settlement, restart, continuation, canonical tags,
   stored blobs, and pacing.
2. A Kemono live public smoke proves the same behavior.

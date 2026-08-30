# PBI-620: Certify Coomer And Kemono CDN Publication

## Observed Gap

Pawchive, Coomer, and Kemono are native providers with fixture coverage. Pawchive has passed the
durable live certification. Coomer and Kemono metadata endpoints were reachable on 2026-08-30, but
their production-path runs did not return media bytes within the bounded 120-second certification
window. Both runs cancelled cleanly without publishing false success, but complete live publication
is not yet proven. The same bounded production-path probe was repeated on 2026-08-30 after the
reset/duplicate verifier was strengthened; both providers again reached the 120-second media wait,
returned their query to pending on cancellation, and produced no certification report or root.

## Required Behavior

- Repeat the Coomer and Kemono public live certification from a host that can retrieve their media
  CDN bytes.
- Do not add a provider-specific scheduler, downloader, timeout bypass, or authentication surface to
  compensate for an external CDN outage.

## Acceptance

1. A Coomer live public smoke proves serial settlement, restart, continuation, canonical tags,
   stored blobs, and pacing.
2. A Kemono live public smoke proves the same behavior.

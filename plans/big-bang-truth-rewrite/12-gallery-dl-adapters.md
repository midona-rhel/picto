# Gallery-dl Adapters

## Purpose

Define site-specific ingestion logic as adapter code, not domain semantics.

## Current Truth

- gallery-dl integration does real work, but site-specific metadata parsing and namespace mapping are too easy to mistake for global tag semantics.

## Target Truth

- gallery-dl is an adapter under subscriptions.
- Site-specific tag extraction, metadata normalization, and credential wiring live here.
- Per-site throttle is global by canonical site identity.
- Default throttle is one active external request window per site per second.

## Rename Map

- keep `gallery-dl`
- rename any user-facing “site metadata logic” phrasing to “adapter logic”

## Delete List

- Delete product docs that treat Danbooru/Pixiv/E621 parsing as global tag behavior.
- Delete duplicate site-specific parsing outside the adapter layer.

## DTOs and Commands Involved

- subscription run commands
- adapter output DTO that feeds import pipeline
- credential lookup and validation commands

## Workflows

- Subscription run starts -> canonical site id resolved -> throttle gate acquired.
- gallery-dl fetches metadata and files -> adapter parses metadata -> import pipeline receives canonical payload.
- Failures are classified once and reported through runtime tasks.

## Acceptance Criteria

- Site-specific logic is isolated under subscriptions.
- Global per-site throttling is documented and enforced.
- Tag coercion from remote metadata is described as adapter behavior only.

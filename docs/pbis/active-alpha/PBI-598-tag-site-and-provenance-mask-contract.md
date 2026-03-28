# PBI-598: Tag site-support and provenance mask contract

## Priority
P2

## AI-generated caveat
This document is based on current product direction clarified during review. It is intentionally narrow. The goal is to lock a simple tag-site/provenance model now without dragging alias-driven site query translation into scope too early.

## Lifecycle
- `Implemented` when the public contract, mask layout, and authority rules are written and linked.
- `Activatable` when `PBI-567` and `PBI-568` are implemented enough that canonical tag and entity-tag storage can own these fields.
- `Activated` when live canonical tag assignment and tag editing paths use this model by default.
- `Legacy removed` when replaced legacy tag-source assumptions and one-off provenance fields are deleted for the activated slice.

Activation depends on:
- [PBI-567-greenfield-library-database-reset.md](./docs/pbis/active-alpha/PBI-567-greenfield-library-database-reset.md)
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-578-bulk-entity-target-and-selection-reset.md](./docs/pbis/active-alpha/PBI-578-bulk-entity-target-and-selection-reset.md)

## Problem
The tag model does not yet encode two product questions cleanly:

- which supported sites are known to use or accept this tag concept
- where a specific tag assignment came from

Without an explicit contract, future subscription/query work will have to guess whether a tag is usable on a site, and import/manual/AI tag assignment history will stay vague or fragmented.

Current risks:
- tag provenance gets mixed up with alias logic before either model is stable
- tag-level site support gets inferred from current entity usage and becomes unstable when relations are removed
- assignment provenance gets overdesigned into several fields before the real product need is locked
- future subscription query building has no stable “is this tag usable on this site?” signal

## Product model to encode
This PBI locks a small, explicit model:

- tags have a curated site-support mask
- entity-tag relations have one provenance mask
- both masks use the same shared `u64` bit registry
- low bits are reserved for provenance class
- high bits are reserved for site bits

This model is intentionally enough for:
- future subscription/query filtering by supported site
- import/manual/AI provenance on tag assignment
- future alias or site-query mapping work to build on top later

This model is intentionally not enough for:
- site-specific outbound query text translation
- alias-driven tag vocabulary mapping
- full provenance history/event log

Those are separate follow-up concerns.

## Locked decisions

### 1. Two authoritative masks, not more
Use exactly these authoritative fields:

- `tag.site_mask: u64`
- `entity_tag.provenance_mask: u64`

Do not add separate `source_site_mask` and `source_flags` fields in this slice.

### 2. `tag.site_mask` means concept-level site support
`tag.site_mask` answers:

- which supported sites are known to use, accept, or be queryable for this tag concept

Rules:
- it is curated concept metadata
- it is not derived from current `entity_tag` rows
- deleting the last assignment imported from one site must not clear that site bit automatically

### 3. `entity_tag.provenance_mask` means assignment provenance
`entity_tag.provenance_mask` answers:

- where this specific tag assignment came from

This includes both:
- provenance class bits such as manual or AI
- site bits such as e621 or gelbooru

Examples:
- manual-only assignment: `MANUAL`
- AI-only assignment: `AI`
- imported from e621: `E621`
- imported from gelbooru and then manually confirmed: `GELBOORU | MANUAL`

### 4. One shared bit registry
The project should define one shared bit registry for this domain and use it consistently across backend and frontend contracts.

Locked layout:
- low bits are reserved for provenance class bits
- high bits are reserved for site bits
- middle bits remain reserved for future expansion

Recommended first layout:
- bits `0..=7`: provenance class
- bits `56..=63`: supported sites first

Example provenance bits:
- `MANUAL`
- `AI`
- `UNKNOWN`
- `LOCAL_TOOL`

Example site bits:
- `E621`
- `GELBOORU`
- `DANBOORU`
- `RULE34`

The exact initial site list can stay small. Reserve the layout now, expand the registry later.

### 5. Tag support and assignment provenance are separate concerns
Do not overload `tag.site_mask` to mean “we have seen this tag imported from these sites on current entities.”

Do not overload `entity_tag.provenance_mask` to mean “this tag concept is valid on these sites in general.”

The split is:
- tag root = concept support
- relation = assignment source

### 6. No mapping layer in this PBI
Do not add:
- site-specific query text mapping
- site-specific alias translation
- outbound subscription query vocabulary tables

That can be added later if needed.

This PBI only guarantees a stable support/provenance base to build on.

## Canonical shapes

### Tag root
Use a field such as:

```rust
pub struct TagInfo {
    pub tag_id: i64,
    pub namespace: Option<String>,
    pub subtag: String,
    pub site_mask: u64,
}
```

Database field:

```sql
tag.site_mask INTEGER NOT NULL DEFAULT 0
```

### Entity-tag relation
Use a field such as:

```rust
pub struct EntityTagRow {
    pub entity_id: i64,
    pub tag_id: i64,
    pub provenance_mask: u64,
}
```

Database field:

```sql
entity_tag.provenance_mask INTEGER NOT NULL DEFAULT 0
```

## Relationship to subscriptions and aliases
This PBI deliberately stops short of query translation.

What it enables later:
- subscriptions can filter candidate tags by `tag.site_mask`
- alias work can later decide how to translate canonical tags into site-specific query terms

What it does not solve:
- whether `female` on one site should query as `1girl` on another
- whether aliases are sufficient for outbound site vocabulary

Those are future PBIs. Do not sneak them into this one.

## Relationship to other reset PBIs
- [PBI-567-greenfield-library-database-reset.md](./docs/pbis/active-alpha/PBI-567-greenfield-library-database-reset.md) owns canonical storage for `tag` and `entity_tag`
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md) owns canonical mutation/read APIs that should expose and update these masks
- [PBI-578-bulk-entity-target-and-selection-reset.md](./docs/pbis/active-alpha/PBI-578-bulk-entity-target-and-selection-reset.md) owns bulk entity targeting for tag assignment mutation
- subscriptions may use this later, but subscription query vocabulary mapping is not part of this contract

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This PBI is complete only when:
- canonical `tag` storage includes `site_mask`
- canonical `entity_tag` storage includes `provenance_mask`
- one shared bit registry exists and is referenced consistently
- `tag.site_mask` is treated as curated concept metadata, not as a derived aggregate from current entity relations
- tag assignment APIs can set `entity_tag.provenance_mask`
- the contract explicitly rejects site-query mapping as out of scope for this slice

## Tests
Required tests:
- tag create/update persists `site_mask`
- entity-tag assignment persists `provenance_mask`
- deleting an `entity_tag` row does not mutate `tag.site_mask`
- bulk tag application can set one provenance mask across many entities
- serialization tests prove the public contract preserves full `u64` values without silent truncation


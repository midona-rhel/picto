# Naming Map

## Purpose

Lock one truthful vocabulary and delete misleading names.

## Current Truth

- Current naming mixes storage terms, UI terms, legacy hydrus terms, and transitional command names.
- `sibling`, `parent`, `flow`, and many `file*` names do not describe the intended model clearly.

## Target Truth

- Code, docs, DTOs, commands, and tests use one canonical vocabulary.
- Legacy names do not survive the final merge.

## Rename Map

- `file` command or DTO meaning logical item -> `media_entity`
- `get_file_all_metadata` -> `get_media_entity_metadata`
- `file-imported` -> `media-entity-imported`
- `flow` -> `subscription_group`
- `get_flows` -> `get_subscription_groups`
- `run_flow` -> `run_subscription_group`
- `sibling` -> `alias`
- `get_tag_siblings_for_tag` -> `get_tag_aliases`
- `setAlias` remains acceptable; `removeAlias` remains acceptable
- `parent` -> `implication`
- `get_tag_parents_for_tag` -> `get_tag_implications`
- `add_tag_parent` -> `add_tag_implication`
- `remove_tag_parent` -> `remove_tag_implication`
- `PTR` product-facing label -> no public replacement; internal name becomes `secondary_tag_db`

## Delete List

- Delete dual naming in docs.
- Delete compatibility aliases after all consumers are moved.
- Delete old generated command names once the backend command surface is renamed.

## DTOs and Commands Involved

- `src/platform/api.ts`
- `src/shared/types/api/core.ts`
- `src/shared/types/generated/commands/*`
- `core/src/dispatch/typed/*`

## Workflows

- Rename commands and DTOs first in docs.
- Rename backend command handlers and generated types next.
- Rename frontend API surface and consumers in the same branch.
- Remove old names before merge.

## Acceptance Criteria

- No visible UI says `PTR`, `sibling`, or `parent`.
- No public command or DTO in the live app uses the old names.
- Search for old names only finds internal appendix docs or deleted-history references.

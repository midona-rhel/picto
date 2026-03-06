# PBI Backlog (Alpha Reset)

This folder was reset for the first alpha release.

## Structure

1. `active-alpha/`: PBIs that are currently release-relevant for `v0.1.0-alpha.*`
2. `archive/`: PBIs retained for history, not currently in alpha release scope

## Active Alpha PBIs (6)

1. [PBI-162](./active-alpha/PBI-162-import-and-subscription-entity-grouping.md)
2. [PBI-166](./active-alpha/PBI-166-feature-first-frontend-folder-realignment.md)
3. [PBI-200](./active-alpha/PBI-200-site-metadata-validation-danbooru.md)
4. [PBI-220](./active-alpha/PBI-220-site-metadata-validation-rule34xxx.md)
5. [PBI-221](./active-alpha/PBI-221-site-metadata-validation-e621.md)
6. [PBI-224](./active-alpha/PBI-224-site-metadata-validation-framework-and-api-contract.md)

## Archived PBIs (49)

All other `PBI-*.md` files moved to `./archive/`.

Use this command to list archived items:

```bash
ls docs/pbis/archive/PBI-*.md | sed 's|docs/pbis/archive/||'
```

## Notes

1. This reset is for alpha release execution clarity, not for deleting backlog history.
2. Archived PBIs can be promoted back to `active-alpha/` if they become release-critical.

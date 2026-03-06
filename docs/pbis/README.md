# PBI Backlog (Alpha Reset)

This folder was reset for the first alpha release.

## Structure

1. `active-alpha/`: PBIs that are currently release-relevant for `v0.1.0-alpha.*`
2. `archive/`: PBIs retained for history, not currently in alpha release scope

## Active Alpha PBIs (16)

1. [PBI-087](./active-alpha/PBI-087-centralize-frontend-domain-mutations-and-refresh-policy.md)
2. [PBI-088](./active-alpha/PBI-088-unify-task-event-ingestion-into-single-runtime-store.md)
3. [PBI-092](./active-alpha/PBI-092-share-runtime-event-schema-between-core-and-frontend.md)
4. [PBI-095](./active-alpha/PBI-095-unify-image-decode-qos-across-grid-detail-and-quicklook.md)
5. [PBI-098](./active-alpha/PBI-098-merge-detailview-detailwindow-quicklook-into-shared-viewer-core.md)
6. [PBI-099](./active-alpha/PBI-099-centralize-context-menu-action-registries-across-domains.md)
7. [PBI-102](./active-alpha/PBI-102-unify-empty-loading-error-state-composition.md)
8. [PBI-143](./active-alpha/PBI-143-color-palette-extraction-and-color-search.md)
9. [PBI-162](./active-alpha/PBI-162-import-and-subscription-entity-grouping.md)
10. [PBI-163](./active-alpha/PBI-163-standardize-runtime-product-namespace.md)
11. [PBI-165](./active-alpha/PBI-165-unify-media-entity-dto-naming.md)
12. [PBI-166](./active-alpha/PBI-166-feature-first-frontend-folder-realignment.md)
13. [PBI-200](./active-alpha/PBI-200-site-metadata-validation-danbooru.md)
14. [PBI-220](./active-alpha/PBI-220-site-metadata-validation-rule34xxx.md)
15. [PBI-221](./active-alpha/PBI-221-site-metadata-validation-e621.md)
16. [PBI-224](./active-alpha/PBI-224-site-metadata-validation-framework-and-api-contract.md)

## Archived PBIs (49)

All other `PBI-*.md` files moved to `./archive/`.

Use this command to list archived items:

```bash
ls docs/pbis/archive/PBI-*.md | sed 's|docs/pbis/archive/||'
```

## Notes

1. This reset is for alpha release execution clarity, not for deleting backlog history.
2. Archived PBIs can be promoted back to `active-alpha/` if they become release-critical.

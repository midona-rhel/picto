# Active PBIs

This directory contains executable work only. Git history retains completed and superseded work.

## Release blockers

1. [PBI-529-frontend-notifications-and-error-feedback.md](PBI-529-frontend-notifications-and-error-feedback.md)
   - consolidate actionable failures and background completion messages
2. [PBI-603-release-finalization-and-integration-gate.md](PBI-603-release-finalization-and-integration-gate.md)
   - run the clean packaged release gate after feature closure

## Product gaps

1. [PBI-605-ai-tagging-activation.md](PBI-605-ai-tagging-activation.md)
   - revalidate optional model lifecycle hardening after the alpha rather than blocking it
2. [PBI-608-complete-grid-context-actions.md](PBI-608-complete-grid-context-actions.md)
   - add real Open With Other, batch rename, and last-used-folder handlers
3. [PBI-609-complete-folder-context-operations.md](PBI-609-complete-folder-context-operations.md)
   - add a canonical multi-scope export target for folder and smart-folder selections
4. [PBI-611-complete-applicable-grid-filter-contract.md](PBI-611-complete-applicable-grid-filter-contract.md)
   - add canonical date facets and palette-aware color-similarity semantics
5. [PBI-612-add-in-library-image-similarity-filter.md](PBI-612-add-in-library-image-similarity-filter.md)
   - add a backend-owned in-library visual-similarity filter rather than faking external search
6. [PBI-614-centralize-theme-runtime-and-preview.md](PBI-614-centralize-theme-runtime-and-preview.md)
   - replace competing persisted/local/document theme state with one preview-capable runtime
7. [PBI-613-centralize-renderer-shortcut-ownership.md](PBI-613-centralize-renderer-shortcut-ownership.md)
   - replace scattered window listeners with one shortcut dispatcher and scoped suspension leases
8. [PBI-615-unify-settings-surface-and-save-model.md](PBI-615-unify-settings-surface-and-save-model.md)
   - unify Settings visual primitives and make its Save/Reset contract truthful
9. [PBI-616-complete-product-actions.md](PBI-616-complete-product-actions.md)
   - complete the approved grid, folder, tag, sidebar, viewer, burger-menu, and library actions

## Backlog policy

- A PBI must describe an observed product gap and a finite acceptance test.
- Generic audits and architecture manifestos are not active PBIs.
- Unreproduced bug buckets and legacy-parity programs are deleted, not carried as release work.
- Do not create one ticket per subscription site. Visible source support is one tested registry.
- When implementation lands, delete the PBI in the same commit.
- If a bug recurs, create a new ticket containing current evidence rather than restoring an old
  plan written against deleted code.

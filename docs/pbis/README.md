# PBI Backlog (Alpha Reset)

This folder was reset for the first alpha release.

## Structure

1. `active-alpha/`: PBIs that are currently release-relevant for `v0.1.0-alpha.*`
2. `archive/`: PBIs retained for history, not currently in alpha release scope

## Active Alpha PBIs (34)

1. [PBI-162](./active-alpha/PBI-162-import-and-subscription-entity-grouping.md)
2. [PBI-200](./active-alpha/PBI-200-site-metadata-validation-danbooru.md)
3. [PBI-220](./active-alpha/PBI-220-site-metadata-validation-rule34xxx.md)
4. [PBI-221](./active-alpha/PBI-221-site-metadata-validation-e621.md)
5. [PBI-224](./active-alpha/PBI-224-site-metadata-validation-framework-and-api-contract.md)
6. [PBI-225](./active-alpha/PBI-225-drag-and-drop-items-into-folders.md)
7. [PBI-226](./active-alpha/PBI-226-smooth-scroll-and-zoom.md)
8. [PBI-227](./active-alpha/PBI-227-first-run-onboarding-and-library-creation-guidance.md)
9. [PBI-228](./active-alpha/PBI-228-local-folder-import-workflow.md)
10. [PBI-229](./active-alpha/PBI-229-subscription-panel-ux-clarity.md)
11. [PBI-231](./active-alpha/PBI-231-windows-collection-and-reorder-fixes.md)
12. [PBI-232](./active-alpha/PBI-232-theme-selector-single-click.md)
13. [PBI-233](./active-alpha/PBI-233-rust-core-domain-folder-realignment.md)
14. [PBI-234](./active-alpha/PBI-234-typed-dispatch-contract-between-core-and-frontend.md)
15. [PBI-235](./active-alpha/PBI-235-deduplicate-mutation-impact-construction.md)
16. [PBI-236](./active-alpha/PBI-236-merge-files-review-into-files-lifecycle.md)
17. [PBI-237](./active-alpha/PBI-237-rename-files-module-to-media-processing.md)
18. [PBI-238](./active-alpha/PBI-238-unify-tag-parsing-paths.md)
19. [PBI-239](./active-alpha/PBI-239-core-module-documentation-and-contract-comments.md)
20. [PBI-240](./active-alpha/PBI-240-rust-core-full-codebase-audit-for-cleanup-pbis.md)
21. [PBI-241](./active-alpha/PBI-241-frontend-full-codebase-audit-for-cleanup-pbis.md)
22. [PBI-242](./active-alpha/PBI-242-clean-up-project-root-folder.md)
23. [PBI-243](./active-alpha/PBI-243-canvas-redraw-policy-on-window-resize.md)
24. [PBI-244](./active-alpha/PBI-244-controller-driven-view-transition-lifecycle.md)
25. [PBI-245](./active-alpha/PBI-245-blurhash-first-transition-loading-strategy.md)
26. [PBI-246](./active-alpha/PBI-246-add-to-folder-modal-with-tree-view.md)
27. [PBI-247](./active-alpha/PBI-247-sidebar-tree-view-needs-folder-icon-or-visual.md)
28. [PBI-248](./active-alpha/PBI-248-unify-context-menu-bulk-selection-actions.md)
29. [PBI-249](./active-alpha/PBI-249-inspector-scrollbar-shifts-content-layout.md)
30. [PBI-250](./active-alpha/PBI-250-import-button-broken-on-linux.md)
31. [PBI-251](./active-alpha/PBI-251-import-progress-indicator.md)
32. [PBI-252](./active-alpha/PBI-252-subscription-setup-help-text-and-query-guidance.md)
33. [PBI-253](./active-alpha/PBI-253-library-search-returns-no-results.md)
34. [PBI-254](./active-alpha/PBI-254-user-guide-in-readme-or-docs.md)

## Archived PBIs (53)

All other `PBI-*.md` files moved to `./archive/`.

Use this command to list archived items:

```bash
ls docs/pbis/archive/PBI-*.md | sed 's|docs/pbis/archive/||'
```

## Notes

1. This reset is for alpha release execution clarity, not for deleting backlog history.
2. Archived PBIs can be promoted back to `active-alpha/` if they become release-critical.

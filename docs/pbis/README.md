# PBI Backlog (Alpha Reset)
This folder tracks the current active alpha backlog.

## Structure

1. `active-alpha/`: PBIs that are currently active
2. `archive/`: PBIs kept for history or deferred work

## Active Alpha PBIs (58)

1. [PBI-162](./active-alpha/PBI-162-import-and-subscription-entity-grouping.md)
2. [PBI-200](./active-alpha/PBI-200-site-metadata-validation-danbooru.md)
3. [PBI-201](./active-alpha/PBI-201-site-metadata-validation-3dbooru.md)
4. [PBI-202](./active-alpha/PBI-202-site-metadata-validation-artstation.md)
5. [PBI-203](./active-alpha/PBI-203-site-metadata-validation-sankaku.md)
6. [PBI-204](./active-alpha/PBI-204-site-metadata-validation-idolcomplex.md)
7. [PBI-205](./active-alpha/PBI-205-site-metadata-validation-twitter.md)
8. [PBI-206](./active-alpha/PBI-206-site-metadata-validation-deviantart.md)
9. [PBI-207](./active-alpha/PBI-207-site-metadata-validation-patreon.md)
10. [PBI-208](./active-alpha/PBI-208-site-metadata-validation-nijie.md)
11. [PBI-209](./active-alpha/PBI-209-site-metadata-validation-tumblr.md)
12. [PBI-210](./active-alpha/PBI-210-site-metadata-validation-fantia.md)
13. [PBI-211](./active-alpha/PBI-211-site-metadata-validation-fanbox.md)
14. [PBI-212](./active-alpha/PBI-212-site-metadata-validation-webtoons.md)
15. [PBI-213](./active-alpha/PBI-213-site-metadata-validation-kemono-party.md)
16. [PBI-214](./active-alpha/PBI-214-site-metadata-validation-coomer-party.md)
17. [PBI-215](./active-alpha/PBI-215-site-metadata-validation-seiso-party.md)
18. [PBI-216](./active-alpha/PBI-216-site-metadata-validation-baraag.md)
19. [PBI-217](./active-alpha/PBI-217-site-metadata-validation-pawoo.md)
20. [PBI-218](./active-alpha/PBI-218-site-metadata-validation-hentaifoundry.md)
21. [PBI-219](./active-alpha/PBI-219-site-metadata-validation-yandere.md)
22. [PBI-220](./active-alpha/PBI-220-site-metadata-validation-rule34xxx.md)
23. [PBI-221](./active-alpha/PBI-221-site-metadata-validation-e621.md)
24. [PBI-222](./active-alpha/PBI-222-site-metadata-validation-furaffinity.md)
25. [PBI-223](./active-alpha/PBI-223-site-metadata-validation-instagram.md)
26. [PBI-224](./active-alpha/PBI-224-site-metadata-validation-framework-and-api-contract.md)
27. [PBI-225](./active-alpha/PBI-225-drag-and-drop-items-into-folders.md)
28. [PBI-226](./active-alpha/PBI-226-smooth-scroll-and-zoom.md)
29. [PBI-227](./active-alpha/PBI-227-first-run-onboarding-and-library-creation-guidance.md)
30. [PBI-228](./active-alpha/PBI-228-local-folder-import-workflow.md)
31. [PBI-229](./active-alpha/PBI-229-subscription-panel-ux-clarity.md)
32. [PBI-231](./active-alpha/PBI-231-windows-collection-and-reorder-fixes.md)
33. [PBI-232](./active-alpha/PBI-232-theme-selector-single-click.md)
34. [PBI-233](./active-alpha/PBI-233-rust-core-domain-folder-realignment.md)
35. [PBI-240](./active-alpha/PBI-240-rust-core-full-codebase-audit-for-cleanup-pbis.md)
36. [PBI-244](./active-alpha/PBI-244-controller-driven-view-transition-lifecycle.md)
37. [PBI-245](./active-alpha/PBI-245-blurhash-first-transition-loading-strategy.md)
38. [PBI-246](./active-alpha/PBI-246-add-to-folder-modal-with-tree-view.md)
39. [PBI-250](./active-alpha/PBI-250-import-dialog-and-picker-reliability.md)
40. [PBI-252](./active-alpha/PBI-252-subscription-setup-help-text-and-query-guidance.md)
41. [PBI-254](./active-alpha/PBI-254-user-guide-in-readme-or-docs.md)
42. [PBI-340](./active-alpha/PBI-340-backend-top-level-module-tree-restructure.md)
43. [PBI-341](./active-alpha/PBI-341-backend-domain-folderization-by-ownership-cluster.md)
44. [PBI-350](./active-alpha/PBI-350-backend-topology-enforcement-and-ci-guardrails.md)
45. [PBI-500](./active-alpha/PBI-500-big-bang-truth-rewrite-and-code-deletion-campaign.md)
46. [PBI-501](./active-alpha/PBI-501-canonical-naming-break.md)
47. [PBI-502](./active-alpha/PBI-502-renderer-boundary-collapse.md)
48. [PBI-503](./active-alpha/PBI-503-runtime-contract-purge.md)
49. [PBI-504](./active-alpha/PBI-504-frontend-state-topology-reset.md)
50. [PBI-507](./active-alpha/PBI-507-tags-domain-rewrite.md)
51. [PBI-508](./active-alpha/PBI-508-folders-and-smart-folders-simplification.md)
52. [PBI-509](./active-alpha/PBI-509-grid-and-scope-model-unification.md)
53. [PBI-510](./active-alpha/PBI-510-sidebar-and-navigation-read-model.md)
54. [PBI-511](./active-alpha/PBI-511-inspector-and-metadata-consolidation.md)
55. [PBI-512](./active-alpha/PBI-512-subscriptions-and-gallery-dl-simplification.md)
56. [PBI-514](./active-alpha/PBI-514-app-shell-and-shared-layer-deletion.md)
57. [PBI-515](./active-alpha/PBI-515-css-and-ui-primitive-consolidation.md)
58. [PBI-516](./active-alpha/PBI-516-test-strategy-rewrite.md)

## Archived PBIs

Archived backlog items can be reintroduced if they become active again.

Use this command to list archived items:

```bash
ls docs/pbis/archive/PBI-*.md | sed 's|docs/pbis/archive/||'
```

## Notes

1. Implemented PBIs should be removed from the backlog.
2. `docs/pbis/README.md` should only reference files that currently exist on disk.

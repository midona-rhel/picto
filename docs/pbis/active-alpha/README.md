# Active PBIs

This directory contains executable work only. Git history retains completed and superseded work.

Active post-release implementation work:

- [`PBI-618-native-onlyfans-coverage.md`](PBI-618-native-onlyfans-coverage.md)
- [`PBI-619-duplicate-review-selection-stability.md`](PBI-619-duplicate-review-selection-stability.md)
- [`PBI-620-restore-missing-subscription-providers.md`](PBI-620-restore-missing-subscription-providers.md)
- [`PBI-621-subscription-merge-smart-names.md`](PBI-621-subscription-merge-smart-names.md)
- [`PBI-622-minimum-text-search-length.md`](PBI-622-minimum-text-search-length.md)

Platform packaging and smoke results remain release-workflow gates rather than product PBIs.

## Backlog policy

- A PBI must describe an observed product gap and a finite acceptance test.
- Generic audits and architecture manifestos are not active PBIs.
- Unreproduced bug buckets and legacy-parity programs are deleted, not carried as release work.
- Do not create one ticket per subscription site. Visible source support is one tested registry.
- When implementation lands, delete the PBI in the same commit.
- If a bug recurs, create a new ticket containing current evidence rather than restoring an old
  plan written against deleted code.

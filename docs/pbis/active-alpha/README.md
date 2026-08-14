# Active PBIs

This directory contains executable work only. Historical reset plans, completed work,
superseded audits, and site-by-site subscription notes live in `docs/pbis/archive/`.

## Release blockers

1. [PBI-603-release-finalization-and-integration-gate.md](PBI-603-release-finalization-and-integration-gate.md)
   - keep the complete verification lane green and prove release packaging
2. [PBI-529-frontend-notifications-and-error-feedback.md](PBI-529-frontend-notifications-and-error-feedback.md)
   - finish one shared non-modal path for actionable operation failures
4. [PBI-605-ai-tagging-activation.md](PBI-605-ai-tagging-activation.md)
   - prove model download, inference, review, apply, and import automation end to end
5. [PBI-602-multi-device-sync-architecture.md](PBI-602-multi-device-sync-architecture.md)
   - finish safe two-device folder sync and restart recovery
6. [PBI-227-first-run-onboarding-and-library-creation-guidance.md](PBI-227-first-run-onboarding-and-library-creation-guidance.md)
   - make first launch lead directly to creating a library

## Backlog policy

- A PBI must describe an observed product gap and a finite acceptance test.
- Generic audits and architecture manifestos are not active PBIs.
- Unreproduced bug buckets and legacy-parity programs are deleted, not carried as release work.
- Do not create one ticket per subscription site. Site support is one tested matrix owned
  by PBI-575.
- When implementation lands, archive the PBI in the same commit.
- If a bug recurs after archival, create a new ticket containing current evidence rather
  than restoring an old plan written against deleted code.

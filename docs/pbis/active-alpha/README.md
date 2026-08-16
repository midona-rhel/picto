# Active PBIs

This directory contains executable work only. Git history retains completed and superseded work.

## Release blockers

1. [PBI-605-ai-tagging-activation.md](PBI-605-ai-tagging-activation.md)
   - prove model download, inference, review, apply, and import automation end to end
2. [PBI-529-frontend-notifications-and-error-feedback.md](PBI-529-frontend-notifications-and-error-feedback.md)
   - consolidate actionable failures and background completion messages
3. [PBI-603-release-finalization-and-integration-gate.md](PBI-603-release-finalization-and-integration-gate.md)
   - run the clean packaged release gate after feature closure

## Backlog policy

- A PBI must describe an observed product gap and a finite acceptance test.
- Generic audits and architecture manifestos are not active PBIs.
- Unreproduced bug buckets and legacy-parity programs are deleted, not carried as release work.
- Do not create one ticket per subscription site. Visible source support is one tested registry.
- When implementation lands, delete the PBI in the same commit.
- If a bug recurs, create a new ticket containing current evidence rather than restoring an old
  plan written against deleted code.

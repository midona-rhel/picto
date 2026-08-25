# Active PBIs

This directory contains executable work only. Git history retains completed and superseded work.

## Release verification

These PBIs contain no new product implementation. They close when the current release candidate
passes its automated and packaged application smokes.

1. [PBI-603-release-finalization-and-integration-gate.md](PBI-603-release-finalization-and-integration-gate.md)
   - run the clean packaged release gate after feature closure
2. [PBI-606-backend-replacement.md](PBI-606-backend-replacement.md)
   - close the replacement only after the packaged fresh-library smoke passes

## Deferred optional work

This section does not block the current release.

1. [PBI-616-complete-product-actions.md](PBI-616-complete-product-actions.md)
   - implement or explicitly drop the remaining optional deep-link, bulk, and library actions

## Backlog policy

- A PBI must describe an observed product gap and a finite acceptance test.
- Generic audits and architecture manifestos are not active PBIs.
- Unreproduced bug buckets and legacy-parity programs are deleted, not carried as release work.
- Do not create one ticket per subscription site. Visible source support is one tested registry.
- When implementation lands, delete the PBI in the same commit.
- If a bug recurs, create a new ticket containing current evidence rather than restoring an old
  plan written against deleted code.

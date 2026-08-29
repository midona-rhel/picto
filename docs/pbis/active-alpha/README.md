# Active PBIs

This directory contains executable work only. Git history retains completed and superseded work.

There are no active alpha PBIs. Platform packaging and smoke results are release-workflow gates,
not unfinished product implementation.

## Backlog policy

- A PBI must describe an observed product gap and a finite acceptance test.
- Generic audits and architecture manifestos are not active PBIs.
- Unreproduced bug buckets and legacy-parity programs are deleted, not carried as release work.
- Do not create one ticket per subscription site. Visible source support is one tested registry.
- When implementation lands, delete the PBI in the same commit.
- If a bug recurs, create a new ticket containing current evidence rather than restoring an old
  plan written against deleted code.

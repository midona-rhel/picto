# PBI-619: Duplicate Review Selection Stability

## Observed Gap

When a background duplicate scan publishes newly discovered pairs, the duplicate currently open in
the review screen can be replaced by a different pair at the same array index.

## Required Behavior

- Review identity is the stable unordered file-pair key, never the current list index.
- Refresh retains the open pair and its loaded previews while that pair still exists.
- If the pair is resolved or disappears, navigation selects the deterministic next surviving pair.
- New pairs append or reorder the surrounding list without replacing the active review.

## Acceptance

1. Insert pairs before and after the active pair during a simulated scan; the displayed pair stays
   unchanged.
2. Remove the active pair; exactly one deterministic neighbor becomes active.
3. Preview readiness from the prior pair cannot publish onto the new pair.

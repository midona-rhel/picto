# PBI-615: Unify Settings surface and save model

## Problem

Settings has the correct broad two-column structure but its panels use inconsistent row, card,
typography, and persistence patterns. Some controls save immediately, some only mutate local state,
and the shared Save/Reset footer implies a transaction that does not actually own every change.
This makes both the visual hierarchy and the consequence of closing the window unreliable.

## Contract

- Audit Picto and reference application's shipped Preferences source first, then define one Picto Settings shell:
  stable 200px navigation, one raised content plane, shared header/body/footer geometry, and reusable
  section/card/row/control primitives.
- Use the resolved Picto UI font and theme tokens. Category titles, descriptions, labels, inputs,
  focus states, and disabled states must not invent panel-local typography or colors.
- Reversible preferences use one draft transaction owned by Settings. Save persists one coherent
  draft; Reset restores defaults into the draft; Cancel/close discards it. Immediate external actions
  such as authentication or account removal are presented as actions and never implied to be part of
  the Save transaction.
- Theme editing consumes PBI-614's preview API. Shortcut editing consumes PBI-613's registry and
  persistence path. Settings must not recreate either subsystem.
- Search returns explicit category/row results and opens the correct stable panel without changing
  panel geometry. Persist the last selected category only after the navigation model has one owner.
- Delete panel-specific copies of card, row, select, button, and footer styles once their consumers
  use the shared Settings primitives. Do not add placeholder categories for behavior Picto lacks.

## Verification

- Render tests cover every real category at minimum supported window size in light and dark themes,
  with no overflow, reflowing navigation, or mismatched header/footer geometry.
- Interaction tests prove the transaction boundary for General, Appearance, grid defaults, AI
  Tagging, and shortcut overrides; external account actions remain immediate and explicit.
- Search tests cover category and individual-row results plus keyboard navigation and focus return.
- A source/LOC check proves old panel-specific visual and persistence paths were deleted rather than
  wrapped.
- User verification A/Bs General, Appearance, Shortcuts, AI Tagging, and Cloud Sync before this PBI is
  removed.

Delete this PBI when the acceptance checks pass. Git history is the archive.

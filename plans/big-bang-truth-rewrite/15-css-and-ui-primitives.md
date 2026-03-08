# CSS And UI Primitives

## Purpose

Shrink duplicated styling and repeated UI shells.

## Current Truth

- CSS rules for sidebars, pickers, panel states, list items, and buttons are duplicated across feature modules.
- Overlay and picker behavior is repeated with minor variations.

## Target Truth

- Shared styling is tokenized and minimal.
- Reusable primitives exist for:
  - sidebar row
  - panel shell
  - overlay shell
  - list item
  - icon button
  - search input row
- Feature CSS only handles genuinely unique layout or visuals.

## Rename Map

- no public command renames here
- internal style names should follow primitive intent, not feature history

## Delete List

- Delete one-off CSS modules that only restyle the same row or panel pattern.
- Delete duplicate tag picker and folder picker shells once primitives exist.
- Delete dead CSS tied to removed PTR UI.

## DTOs and Commands Involved

- none

## Workflows

- Extract shared tokens and primitives first.
- Move feature modules onto primitives.
- Delete old CSS modules immediately after the last consumer moves.

## Acceptance Criteria

- Sidebar, picker, and panel styles no longer reimplement the same active, hover, and badge rules in each feature.
- Removed PTR UI leaves no dead CSS.
- Shared UI code is small enough to audit quickly.

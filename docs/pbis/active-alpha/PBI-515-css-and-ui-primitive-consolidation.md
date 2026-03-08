# PBI-515: CSS and UI primitive consolidation

## Priority
P1

## Problem
CSS and UI structure still repeat the same panel, list-row, picker, and button patterns across separate modules.

## Goal
Delete duplicated style logic and establish one clear primitive layer.

## Implementation
1. Establish a CSS token layer.
2. Collapse repeated sidebar, panel, list-row, picker, and button patterns.
3. Remove one-off modules that restyle the same primitives with tiny differences.

## Acceptance Criteria
1. CSS module count drops.
2. Shared primitives exist for cards, rows, modal sections, panels, and list items.

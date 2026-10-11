---
name: Work item
about: Scope a defect, feature, refactor, or performance change you intend to implement.
title: "[P2] "
labels: "priority: P2"
assignees: ""
---

<!-- P2 is the default. If another priority fits, change both the title prefix
and the priority label. Add one area label (backend, sources, renderer, ...).
Search existing issues first and link related work below. -->

## Problem

<!-- The concrete problem, who it affects, and the intended outcome. -->

## Evidence

<!-- Reviewed commit (dev SHA). Link exact source lines or observations.
Separate reproduced behavior, source-verified findings, and hypotheses.
State the checks you actually ran and their limits. A stale doc alone does
not prove a code defect. -->

## Reproduction / test card

### Starting state

<!-- Library size and shape, settings, subscription, files needed. -->

### Actions

1.

### Expected

<!-- Observable behavior. For research or refactors, state the verification
procedure and expected deliverable instead of inventing a repro. -->

### Evidence to report

<!-- Tested head, focused checks, actual vs expected, open limitations. -->

## Implementation boundary

<!-- Scope and exclusions. Name the owner module and existing mechanism to
reuse (publish path, job queue, source adapter, ...). For simplification,
name the superseded paths to delete and report net production-code change
separately from tests/docs. Schema changes: state the migration and backup. -->

## Acceptance criteria

- [ ] <!-- Observable outcome addressing the problem. -->
- [ ] <!-- Regression checks and preserved behavior. -->
- [ ] <!-- Docs updated in their existing home; user acceptance where applicable. -->

## Related

<!-- Dependencies, overlapping issues, prior completed work. -->

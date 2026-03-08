# PBI-504: Frontend state topology reset

## Priority
P0

## Problem
Frontend state still mixes backend-derived facts, feature orchestration, and UI-only concerns in oversized stores and app-shell plumbing.

## Goal
Force frontend state into three buckets only.

## Required State Buckets
1. `runtimeStore` for backend-derived live state and invalidation
2. query or read-model cache for visible scopes
3. `uiStore` for selection, modals, panels, drag, scroll, and temporary form state

## Implementation
1. Remove stores that mix domain truth and UI control without reason.
2. Move app-shell orchestration state out of feature data stores.
3. Keep backend semantics out of UI-only state.

## Acceptance Criteria
1. No single store owns both broad domain truth and broad UI orchestration.
2. App shell state is composition-oriented, not domain-mutating.

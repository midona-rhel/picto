# PBI-510: Sidebar and navigation read model

## Priority
P0

## Problem
Sidebar refresh and navigation semantics still leak business logic into renderer state and feature-specific invalidation code.

## Goal
Make the sidebar a backend read-model consumer and keep navigation thin.

## Implementation
1. Sidebar tree and counts come from backend read models.
2. Navigation state contains only active scope, active view, and history.
3. System views, folders, and smart folders share one sidebar node shape.
4. Remove feature-local sidebar refresh hacks.

## Acceptance Criteria
1. Sidebar refresh derives from mutation receipts and published tree/count read models.
2. Navigation store is thin and free of domain semantics.

# PBI-508: Folders and smart folders simplification

## Priority
P0

## Problem
Stored folder containers and computed smart-folder scopes are still too entangled between backend and renderer.

## Goal
Separate stored containers from computed scopes cleanly.

## Implementation
1. `Folder` remains an ordered hierarchical container with membership and auto-tagging.
2. `SmartFolder` remains a backend-computed predicate scope only.
3. Smart-folder counts and membership are bitmap/query derived.
4. Renderer does not compile smart-folder semantics.

## Acceptance Criteria
1. Folder membership and ordering logic are backend-owned.
2. Smart-folder query logic is backend-owned.
3. Sidebar consumes published read models instead of rebuilding folder logic.

# PBI-092: Share runtime event schema between core and frontend

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Backend defines typed event payload structs in Rust:
   - `/Users/midona/Code/imaginator/core/src/events.rs`
2. Frontend handlers still use ad-hoc payload casting in several places:
   - `/Users/midona/Code/imaginator/src/components/layout/SidebarJobStatus.tsx`
   - `/Users/midona/Code/imaginator/src/components/settings/PtrPanel.tsx`
   - `/Users/midona/Code/imaginator/src/stores/eventBridge.ts`

## Problem
Event payload contracts are not strongly shared across runtime boundary. Frontend casting increases drift risk and makes refactors brittle.

## Scope
- `/Users/midona/Code/imaginator/core/src/events.rs`
- `/Users/midona/Code/imaginator/src/types/api.ts`
- `/Users/midona/Code/imaginator/src/stores/eventBridge.ts`
- `/Users/midona/Code/imaginator/src/components/layout/SidebarJobStatus.tsx`
- `/Users/midona/Code/imaginator/src/components/settings/PtrPanel.tsx`

## Implementation
1. Define canonical event payload schema artifacts from core (generated JSON schema or source-of-truth mapping file).
2. Generate/derive frontend event payload types from that shared contract.
3. Replace `as { ... }` ad-hoc casts in listeners with typed payload decoders/selectors.
4. Add contract tests validating that emitted events conform to expected schema.

## Acceptance Criteria
1. Frontend event listeners consume strongly typed payloads without manual casting.
2. Contract drift between Rust event structs and TS payload interfaces is CI-detected.
3. Event bridge and task/sidebar consumers are resilient to payload evolution.

## Test Cases
1. Schema generation + type-check pass for all runtime events.
2. Breaking payload change in Rust fails frontend contract check.
3. Runtime listener path rejects malformed payloads safely.

## Risk
Low to medium. Tooling and typing improvements with high long-term maintainability benefits.


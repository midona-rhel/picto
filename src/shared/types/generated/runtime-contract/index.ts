/**
 * Runtime contract types — generated from Rust via ts-rs.
 *
 * Individual type files are auto-generated from core/src/runtime_contract/*.rs.
 * ResourceKey is frontend-only (template literal type, no Rust equivalent).
 *
 * Regenerate types: `cargo test --lib`
 */

export type { Domain } from './Domain';
export type { TagChangeDetails } from './TagChangeDetails';
export type { MediaMetadataField } from './MediaMetadataField';
export type { MediaDerivativeField } from './MediaDerivativeField';
export type { StateChanges } from './StateChanges';
export type { StateChangedEvent } from './StateChangedEvent';
export type { RuntimeSnapshot } from './RuntimeSnapshot';
export type { RuntimeTask } from './RuntimeTask';
export type { SidebarCounts } from './SidebarCounts';
export type { TaskKind } from './TaskKind';
export type { TaskProgress } from './TaskProgress';
export type { TaskStatus } from './TaskStatus';
export type { TaskUpsertedEvent } from './TaskUpsertedEvent';
export type { TaskRemovedEvent } from './TaskRemovedEvent';

// ─── Frontend-only types (no Rust equivalent) ──────────────────────────────

export type ResourceKey =
  | 'sidebar/tree'
  | 'sidebar/counts'
  | `grid/${string}`
  | `metadata/hash:${string}`
  | 'selection/current'
  | 'view-prefs/current'
  | 'subscriptions/list';

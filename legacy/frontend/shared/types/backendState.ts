import type { Domain } from './generated/runtime-contract/Domain';
import type { MediaDerivativeField } from './generated/runtime-contract/MediaDerivativeField';
import type { MediaMetadataField } from './generated/runtime-contract/MediaMetadataField';
import type { RuntimeSnapshot } from './generated/runtime-contract/RuntimeSnapshot';
import type { RuntimeTask } from './generated/runtime-contract/RuntimeTask';
import type { SidebarCounts } from './generated/runtime-contract/SidebarCounts';
import type { StateChangedEvent } from './generated/runtime-contract/StateChangedEvent';
import type { StateChanges } from './generated/runtime-contract/StateChanges';
import type { TagChangeDetails } from './generated/runtime-contract/TagChangeDetails';
import type { TaskKind } from './generated/runtime-contract/TaskKind';
import type { TaskProgress } from './generated/runtime-contract/TaskProgress';
import type { TaskRemovedEvent } from './generated/runtime-contract/TaskRemovedEvent';
import type { TaskStatus } from './generated/runtime-contract/TaskStatus';
import type { TaskUpsertedEvent } from './generated/runtime-contract/TaskUpsertedEvent';

/**
 * Frontend boundary types for backend-owned state and runtime events.
 *
 * The files under `generated/runtime-contract/` are raw generated bindings.
 * Frontend code should import from this file instead so the entrypoint name
 * describes what these types are for.
 */

export type {
  Domain,
  MediaDerivativeField,
  MediaMetadataField,
  RuntimeSnapshot,
  RuntimeTask,
  SidebarCounts,
  StateChangedEvent,
  StateChanges,
  TagChangeDetails,
  TaskKind,
  TaskProgress,
  TaskRemovedEvent,
  TaskStatus,
  TaskUpsertedEvent,
};

export type ResourceKey =
  | 'sidebar/tree'
  | 'sidebar/counts'
  | `grid/${string}`
  | `metadata/hash:${string}`
  | 'selection/current'
  | 'view-prefs/current'
  | 'subscriptions/list';

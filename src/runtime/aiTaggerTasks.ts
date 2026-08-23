/**
 * Reads persisted task state for AI surfaces.
 *
 * Task progress is owned by SQLite and queried through `tasks.get`. The
 * renderer does not synthesize task rows or consume removed runtime task
 * events. Library invalidation only tells this reader when to query again.
 */

import { useSyncExternalStore } from 'react';
import { invoke, listen, type UnlistenFn } from '../platform/ipc';
import type { LibraryChanged } from '../shared/types/generated/application/LibraryChanged';
import type { TaskSnapshot } from '../shared/types/generated/application/TaskSnapshot';

export interface AiTaggerTaskState {
  snapshot: TaskSnapshot | null;
  refresh: () => Promise<void>;
}

let snapshot: TaskSnapshot | null = null;
let started = false;
let refreshInFlight: Promise<void> | null = null;
let unlisten: UnlistenFn | undefined;
const subscribers = new Set<() => void>();
let state: AiTaggerTaskState;

function emit(next: TaskSnapshot | null) {
  snapshot = next;
  state = { snapshot, refresh };
  for (const subscriber of subscribers) subscriber();
}

async function refresh(): Promise<void> {
  if (refreshInFlight) return refreshInFlight;
  refreshInFlight = invoke<TaskSnapshot>('tasks.get')
    .then((next) => emit(next))
    .catch(() => {
      // A closed library or shutting-down host is not a durable task error.
    })
    .finally(() => {
      refreshInFlight = null;
    });
  return refreshInFlight;
}

function start() {
  if (started) return;
  started = true;
  void refresh();
  void listen<LibraryChanged>('library/changed', ({ payload }) => {
    if (payload.resources.includes('tasks')) void refresh();
  }).then((remove) => {
    if (!started) remove();
    else unlisten = remove;
  }).catch(() => {});
}

function subscribe(callback: () => void): () => void {
  start();
  subscribers.add(callback);
  return () => subscribers.delete(callback);
}

function getSnapshot(): AiTaggerTaskState { return state; }

export function useAiTaggerTasks(): AiTaggerTaskState {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

export function stopAiTaggerTasksForTests(): void {
  started = false;
  unlisten?.();
  unlisten = undefined;
  snapshot = null;
  state = { snapshot, refresh };
  refreshInFlight = null;
}

state = { snapshot, refresh };

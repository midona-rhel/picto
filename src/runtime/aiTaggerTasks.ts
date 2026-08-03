/**
 * AI tagger runtime task tracking — auto-tag prediction progress and model
 * download progress, fed by `runtime/task_upserted` / `runtime/task_removed`.
 *
 * Listeners start lazily on first subscription so both the main window and
 * the settings window can consume this module independently.
 */

import { useSyncExternalStore } from 'react';
import { listen } from '../platform/ipc';
import type { RuntimeTask } from '../shared/types/generated/runtime-contract/RuntimeTask';

export interface AiTaggerTaskState {
  /** The singleton auto-tag prediction task, while one is running. */
  autoTag: RuntimeTask | null;
  /** Model download tasks keyed by model slug. */
  downloads: Record<string, RuntimeTask>;
}

let state: AiTaggerTaskState = { autoTag: null, downloads: {} };
const subscribers = new Set<() => void>();
let listenersStarted = false;

const DOWNLOAD_PREFIX = 'model_download:';
const AUTO_TAG_ID = 'auto_tag';

function emit(next: AiTaggerTaskState) {
  state = next;
  for (const cb of subscribers) {
    try {
      cb();
    } catch (error) {
      console.error('aiTaggerTasks subscriber failed', error);
    }
  }
}

function startListeners() {
  if (listenersStarted) return;
  listenersStarted = true;

  void listen<{ task?: RuntimeTask }>('runtime/task_upserted', ({ payload }) => {
    const task = payload.task;
    if (!task) return;
    if (task.kind === 'auto_tag') {
      emit({ ...state, autoTag: task });
    } else if (task.kind === 'model_download') {
      const slug = task.task_id.startsWith(DOWNLOAD_PREFIX)
        ? task.task_id.slice(DOWNLOAD_PREFIX.length)
        : task.task_id;
      emit({ ...state, downloads: { ...state.downloads, [slug]: task } });
    }
  });

  void listen<{ task_id?: string }>('runtime/task_removed', ({ payload }) => {
    const id = payload.task_id ?? '';
    if (id === AUTO_TAG_ID) {
      emit({ ...state, autoTag: null });
    } else if (id.startsWith(DOWNLOAD_PREFIX)) {
      const slug = id.slice(DOWNLOAD_PREFIX.length);
      if (slug in state.downloads) {
        const downloads = { ...state.downloads };
        delete downloads[slug];
        emit({ ...state, downloads });
      }
    }
  });
}

function subscribe(cb: () => void): () => void {
  startListeners();
  subscribers.add(cb);
  return () => {
    subscribers.delete(cb);
  };
}

function getSnapshot(): AiTaggerTaskState {
  return state;
}

/** Live auto-tag + model download task state. */
export function useAiTaggerTasks(): AiTaggerTaskState {
  return useSyncExternalStore(subscribe, getSnapshot);
}

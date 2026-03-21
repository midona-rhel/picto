/**
 * Subscription & group progress state — extracted from runtimeSyncStore
 * so the runtime layer stays pure (snapshot + receipts + tasks).
 *
 * Subscribes to runtimeSyncStore's tasksById and derives subscription-
 * specific progress, linger timers, and finished-event projections.
 */

import { create } from 'zustand';
import { useRuntimeSyncStore } from '../../state/runtimeSyncStore';
import { notifyError } from '../../shared/lib/notify';
import { projectRuntimeTasks } from '../../state/runtimeTaskProjection';
import { api } from '#desktop/api';
import { logBestEffortError } from '../../shared/lib/asyncOps';
import type { RuntimeTask } from '../../shared/types/generated/runtime-contract';
import type {
  GroupFinishedEvent,
  GroupProgressEvent,
  SubscriptionFinishedEvent,
} from '../../shared/types/api';

// ---------------------------------------------------------------------------
// Derived types
// ---------------------------------------------------------------------------

export interface RuntimeSubscriptionProgress {
  subscription_id: string;
  subscription_name: string;
  group_name?: string;
  query_id?: string;
  query_name?: string;
  files_downloaded: number;
  files_skipped: number;
  pages_fetched: number;
  status_text: string;
  phase?: string;
  current_post_id?: string | null;
  current_post_items: number;
  posts_processed: number;
  resume_cursor?: string | null;
  last_error?: string | null;
  status: 'running' | 'finished';
  finished_status?: 'succeeded' | 'failed' | 'cancelled';
  failure_kind?: string | null;
  error?: string | null;
}

// ---------------------------------------------------------------------------
// State shape
// ---------------------------------------------------------------------------

export interface SubscriptionProgressState {
  subscriptionProgressById: Map<string, RuntimeSubscriptionProgress>;
  lastSubscriptionFinished: SubscriptionFinishedEvent | null;
  subscriptionEventSeq: number;

  groupProgressById: Map<string, GroupProgressEvent>;
  lastGroupFinished: GroupFinishedEvent | null;
  groupEventSeq: number;

  // Actions
  start: () => void;
  stop: () => void;
  refreshFromBackend: () => Promise<void>;
}

// ---------------------------------------------------------------------------
// Module-level state
// ---------------------------------------------------------------------------

const subFinishedTimers = new Map<string, ReturnType<typeof setTimeout>>();
let runtimeUnsub: (() => void) | null = null;

function clearSubTimers() {
  for (const timer of subFinishedTimers.values()) clearTimeout(timer);
  subFinishedTimers.clear();
}

function lingerMs(task: RuntimeTask): number {
  const detail = task.detail as Record<string, unknown> | undefined;
  const failureKind = detail?.failure_kind;
  if (failureKind === 'inbox_full') return 6000;
  if (task.status === 'failed') return 4500;
  return 2200;
}

function resolveFinishedSubStatusText(event: SubscriptionFinishedEvent): string {
  if (event.status === 'cancelled' && event.failure_kind === 'inbox_full') return 'Paused (Inbox full)';
  if (event.status === 'succeeded') return 'Completed';
  if (event.status === 'cancelled') return 'Cancelled';
  return 'Failed';
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const useSubscriptionProgressStore = create<SubscriptionProgressState>((set, get) => ({
  subscriptionProgressById: new Map(),
  lastSubscriptionFinished: null,
  subscriptionEventSeq: 0,

  groupProgressById: new Map(),
  lastGroupFinished: null,
  groupEventSeq: 0,

  start: () => {
    if (runtimeUnsub) return;

    // Subscribe to runtimeSyncStore — react to any tasksById changes
    let prevTasksById = useRuntimeSyncStore.getState().tasksById;
    runtimeUnsub = useRuntimeSyncStore.subscribe((state) => {
      const tasksById = state.tasksById;
      if (tasksById === prevTasksById) return;

      // Find tasks that changed
      for (const [taskId, task] of tasksById) {
        const prev = prevTasksById.get(taskId);
        if (prev === task) continue;
        applyTaskChange(set, task);
      }

      // Derive group progress from all tasks
      const projection = projectRuntimeTasks(tasksById.values());
      set({ groupProgressById: projection.groupProgressById });

      prevTasksById = tasksById;
    });

    // Seed from backend
    void get().refreshFromBackend();
  },

  stop: () => {
    if (runtimeUnsub) {
      runtimeUnsub();
      runtimeUnsub = null;
    }
    clearSubTimers();
    set({
      subscriptionProgressById: new Map(),
      lastSubscriptionFinished: null,
      subscriptionEventSeq: 0,
      groupProgressById: new Map(),
      lastGroupFinished: null,
      groupEventSeq: 0,
    });
  },

  refreshFromBackend: async () => {
    try {
      const [runningSubscriptionIdsRaw, runningProgress] = await Promise.all([
        api.subscriptions.getRunning(),
        api.subscriptions.getRunningProgress().catch((error) => {
          logBestEffortError('subscriptionProgressStore.runningProgress', error);
          return [];
        }),
      ]);

      set((state) => {
        const runningSubscriptionIds = new Set<string>([
          ...runningSubscriptionIdsRaw,
          ...runningProgress.map((p) => p.subscription_id),
        ]);
        const subscriptionProgressById = new Map(state.subscriptionProgressById);

        for (const [subId, progress] of subscriptionProgressById.entries()) {
          if (progress.status === 'running' && !runningSubscriptionIds.has(subId)) {
            subscriptionProgressById.delete(subId);
          }
        }

        for (const progress of runningProgress) {
          const existing = subscriptionProgressById.get(progress.subscription_id);
          subscriptionProgressById.set(progress.subscription_id, {
            subscription_id: progress.subscription_id,
            subscription_name:
              (progress.subscription_name ?? '').trim()
              || existing?.subscription_name
              || `Subscription ${progress.subscription_id}`,
            query_id: progress.query_id,
            query_name: progress.query_name ?? existing?.query_name,
            files_downloaded: progress.files_downloaded,
            files_skipped: progress.files_skipped,
            pages_fetched: progress.pages_fetched,
            status_text: progress.status_text,
            current_post_items: 0,
            posts_processed: 0,
            resume_cursor: null,
            status: 'running',
          });
        }

        const tasksById = useRuntimeSyncStore.getState().tasksById;
        const taskProjection = projectRuntimeTasks(tasksById.values());

        return {
          subscriptionProgressById,
          groupProgressById: taskProjection.groupProgressById,
        };
      });
    } catch (error) {
      logBestEffortError('subscriptionProgressStore.refreshFromBackend', error);
    }
  },
}));

// ---------------------------------------------------------------------------
// Task change handler (extracted from old runtimeSyncStore.applyTaskUpsert)
// ---------------------------------------------------------------------------

function applyTaskChange(
  set: (fn: (state: SubscriptionProgressState) => Partial<SubscriptionProgressState>) => void,
  task: RuntimeTask,
) {
  const isRunning = task.status === 'running' || task.status === 'cancelling';
  const isTerminal = task.status === 'finished' || task.status === 'failed';

  // Group tasks
  if (task.kind === 'subscription_group') {
    if (isTerminal) {
      set((state) => ({
        groupEventSeq: state.groupEventSeq + 1,
        lastGroupFinished: {
          group_id: task.task_id.replace(/^group:/, ''),
          status: task.status === 'finished' ? 'succeeded' : 'failed',
        } as GroupFinishedEvent,
      }));
    } else {
      set((state) => ({
        groupEventSeq: state.groupEventSeq + 1,
      }));
    }
  }

  // Subscription tasks
  if (task.kind === 'subscription') {
    const detail = task.detail as Record<string, unknown> | undefined;
    if (!detail) return;

    const subId = (detail.subscription_id as string) ?? task.task_id.replace(/^sub:/, '');
    const timer = subFinishedTimers.get(subId);
    if (timer && isRunning) {
      clearTimeout(timer);
      subFinishedTimers.delete(subId);
    }

    set((state) => {
      const subscriptionProgressById = new Map(state.subscriptionProgressById);
      const existing = subscriptionProgressById.get(subId);
      const patch: Partial<SubscriptionProgressState> = {
        subscriptionEventSeq: state.subscriptionEventSeq + 1,
      };

      if (isRunning) {
        subscriptionProgressById.set(subId, {
          subscription_id: subId,
          subscription_name:
            ((detail.subscription_name as string) ?? '').trim()
            || existing?.subscription_name
            || `Subscription ${subId}`,
          group_name: (detail.group_name as string | undefined) ?? existing?.group_name,
          query_id: detail.query_id as string | undefined,
          query_name: (detail.query_name as string | undefined) ?? existing?.query_name,
          files_downloaded: (detail.files_downloaded as number) ?? 0,
          files_skipped: (detail.files_skipped as number) ?? 0,
          pages_fetched: (detail.pages_fetched as number) ?? 0,
          status_text: (detail.status_text as string) ?? 'Running...',
          phase: (detail.phase as string | undefined) ?? existing?.phase,
          current_post_id: (detail.current_post_id as string | undefined) ?? existing?.current_post_id,
          current_post_items: (detail.current_post_items as number) ?? existing?.current_post_items ?? 0,
          posts_processed: (detail.posts_processed as number) ?? existing?.posts_processed ?? 0,
          resume_cursor: (detail.resume_cursor as string | undefined) ?? existing?.resume_cursor,
          last_error: (detail.last_error as string | undefined) ?? existing?.last_error,
          status: 'running',
        });
      } else if (isTerminal) {
        const finishedStatus =
          (detail.finished_status as string)
          ?? (task.status === 'finished' ? 'succeeded' : 'failed');
        subscriptionProgressById.set(subId, {
          subscription_id: subId,
          subscription_name:
            ((detail.subscription_name as string) ?? '').trim()
            || existing?.subscription_name
            || `Subscription ${subId}`,
          group_name: (detail.group_name as string | undefined) ?? existing?.group_name,
          query_id: detail.query_id as string | undefined,
          query_name: (detail.query_name as string | undefined) ?? existing?.query_name,
          files_downloaded: (detail.files_downloaded as number) ?? 0,
          files_skipped: (detail.files_skipped as number) ?? 0,
          pages_fetched: (detail.pages_fetched as number) ?? existing?.pages_fetched ?? 0,
          status_text: (detail.status_text as string) ?? resolveFinishedSubStatusText({
            status: finishedStatus as 'succeeded' | 'failed' | 'cancelled',
            failure_kind: detail.failure_kind as string | undefined,
          } as SubscriptionFinishedEvent),
          phase: (detail.phase as string | undefined) ?? 'finished',
          current_post_id: null,
          current_post_items: 0,
          posts_processed: (detail.posts_processed as number) ?? existing?.posts_processed ?? 0,
          resume_cursor: (detail.resume_cursor as string | undefined) ?? existing?.resume_cursor,
          last_error: (detail.last_error as string | undefined) ?? (detail.error as string | undefined),
          status: 'finished',
          finished_status: finishedStatus as 'succeeded' | 'failed' | 'cancelled',
          failure_kind: detail.failure_kind as string | undefined,
          error: detail.error as string | undefined,
        });
        patch.lastSubscriptionFinished = {
          subscription_id: subId,
          subscription_name: (detail.subscription_name as string) ?? '',
          status: finishedStatus,
          files_downloaded: (detail.files_downloaded as number) ?? 0,
          files_skipped: (detail.files_skipped as number) ?? 0,
          failure_kind: detail.failure_kind as string | undefined,
          error: detail.error as string | undefined,
        } as SubscriptionFinishedEvent;
      }

      patch.subscriptionProgressById = subscriptionProgressById;
      return patch;
    });

    // Notify on auth failure
    if (isTerminal) {
      const fk = (detail?.failure_kind as string) ?? '';
      if (fk === 'unauthorized' || fk === 'expired') {
        const subName = (detail?.subscription_name as string) ?? 'Subscription';
        notifyError(
          `${subName}: credentials expired or invalid. Update them in Subscriptions → Credentials.`,
          'Authentication Failed',
        );
      }
    }

    // Schedule subscription progress cleanup linger
    if (isTerminal) {
      const existingSub = subFinishedTimers.get(subId);
      if (existingSub) clearTimeout(existingSub);
      const subTimer = setTimeout(() => {
        set((state) => {
          const current = state.subscriptionProgressById.get(subId);
          if (!current || current.status !== 'finished') return {};
          const subscriptionProgressById = new Map(state.subscriptionProgressById);
          subscriptionProgressById.delete(subId);
          return { subscriptionProgressById };
        });
        subFinishedTimers.delete(subId);
      }, lingerMs(task));
      subFinishedTimers.set(subId, subTimer);
    }
  }
}

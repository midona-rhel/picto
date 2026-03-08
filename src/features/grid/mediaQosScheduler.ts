/**
 * Shared media QoS scheduler.
 *
 * Single decode/load scheduler used by grid atlas, detail/quicklook preloader,
 * and any future image decode path. Provides lane-based priority, global heavy
 * codec caps, cancellation, and queued task upgrades.
 */

export type MediaQosLane = 'critical' | 'visible' | 'prefetch';

export interface MediaQosTaskHandle {
  id: number;
  cancel: () => void;
  upgrade: (lane: MediaQosLane, priority?: number) => void;
}

interface TaskRecord {
  id: number;
  lane: MediaQosLane;
  priority: number;
  heavy: boolean;
  run: (signal: AbortSignal) => Promise<void> | void;
  controller: AbortController | null;
  cancelled: boolean;
  started: boolean;
  enqueuedAt: number;
}

interface LaneBudget {
  maxActive: number;
}

export interface MediaQosStats {
  activeTotal: number;
  activeHeavy: number;
  queuedTotal: number;
  activeByLane: Record<MediaQosLane, number>;
  queuedByLane: Record<MediaQosLane, number>;
}

const MAX_ACTIVE_TOTAL = 8;
const MAX_ACTIVE_HEAVY = 2;
const LANE_ORDER: MediaQosLane[] = ['critical', 'visible', 'prefetch'];
const LANE_BUDGETS: Record<MediaQosLane, LaneBudget> = {
  critical: { maxActive: 4 },
  visible: { maxActive: 6 },
  prefetch: { maxActive: 3 },
};

let nextTaskId = 1;
const queuedByLane: Record<MediaQosLane, TaskRecord[]> = {
  critical: [],
  visible: [],
  prefetch: [],
};
const activeTasks = new Map<number, TaskRecord>();
const activeByLane: Record<MediaQosLane, number> = {
  critical: 0,
  visible: 0,
  prefetch: 0,
};
let activeTotal = 0;
let activeHeavy = 0;

function sortLaneQueue(lane: MediaQosLane): void {
  const queue = queuedByLane[lane];
  queue.sort((a, b) => {
    if (a.priority !== b.priority) return a.priority - b.priority;
    return a.enqueuedAt - b.enqueuedAt;
  });
}

function removeFromLaneQueue(task: TaskRecord): boolean {
  const queue = queuedByLane[task.lane];
  const idx = queue.findIndex((candidate) => candidate.id === task.id);
  if (idx < 0) return false;
  queue.splice(idx, 1);
  return true;
}

function completeTask(task: TaskRecord): void {
  if (!activeTasks.has(task.id)) return;
  activeTasks.delete(task.id);
  activeTotal = Math.max(0, activeTotal - 1);
  activeByLane[task.lane] = Math.max(0, activeByLane[task.lane] - 1);
  if (task.heavy) activeHeavy = Math.max(0, activeHeavy - 1);
  pump();
}

function tryPickLaneTask(lane: MediaQosLane): TaskRecord | null {
  if (activeByLane[lane] >= LANE_BUDGETS[lane].maxActive) return null;
  const queue = queuedByLane[lane];
  if (queue.length === 0) return null;

  for (let i = 0; i < queue.length; i++) {
    const candidate = queue[i];
    if (candidate.cancelled) {
      queue.splice(i, 1);
      i -= 1;
      continue;
    }
    if (candidate.heavy && activeHeavy >= MAX_ACTIVE_HEAVY) continue;
    queue.splice(i, 1);
    return candidate;
  }

  return null;
}

function startTask(task: TaskRecord): void {
  if (task.cancelled) return;
  task.started = true;
  task.controller = new AbortController();
  activeTasks.set(task.id, task);
  activeTotal += 1;
  activeByLane[task.lane] += 1;
  if (task.heavy) activeHeavy += 1;

  Promise.resolve()
    .then(() => task.run(task.controller!.signal))
    .catch(() => {
      // Best-effort scheduler: individual decode failures must not poison queue.
    })
    .finally(() => {
      completeTask(task);
    });
}

function pump(): void {
  if (activeTotal >= MAX_ACTIVE_TOTAL) return;

  while (activeTotal < MAX_ACTIVE_TOTAL) {
    let next: TaskRecord | null = null;
    for (const lane of LANE_ORDER) {
      next = tryPickLaneTask(lane);
      if (next) break;
    }
    if (!next) break;
    startTask(next);
  }
}

export function enqueueMediaQosTask(options: {
  lane: MediaQosLane;
  run: (signal: AbortSignal) => Promise<void> | void;
  heavy?: boolean;
  priority?: number;
}): MediaQosTaskHandle {
  const task: TaskRecord = {
    id: nextTaskId++,
    lane: options.lane,
    priority: options.priority ?? 0,
    heavy: !!options.heavy,
    run: options.run,
    controller: null,
    cancelled: false,
    started: false,
    enqueuedAt: performance.now(),
  };

  queuedByLane[task.lane].push(task);
  sortLaneQueue(task.lane);
  pump();

  return {
    id: task.id,
    cancel: () => {
      if (task.cancelled) return;
      task.cancelled = true;
      if (task.started) {
        task.controller?.abort();
      } else {
        removeFromLaneQueue(task);
      }
    },
    upgrade: (lane: MediaQosLane, priority?: number) => {
      if (task.cancelled || task.started) return;
      if (lane !== task.lane) {
        removeFromLaneQueue(task);
        task.lane = lane;
        queuedByLane[task.lane].push(task);
      }
      if (typeof priority === 'number') task.priority = priority;
      sortLaneQueue(task.lane);
      pump();
    },
  };
}

export function getMediaQosStats(): MediaQosStats {
  return {
    activeTotal,
    activeHeavy,
    queuedTotal:
      queuedByLane.critical.length
      + queuedByLane.visible.length
      + queuedByLane.prefetch.length,
    activeByLane: {
      critical: activeByLane.critical,
      visible: activeByLane.visible,
      prefetch: activeByLane.prefetch,
    },
    queuedByLane: {
      critical: queuedByLane.critical.length,
      visible: queuedByLane.visible.length,
      prefetch: queuedByLane.prefetch.length,
    },
  };
}

export function resetMediaQosSchedulerForTests(): void {
  for (const task of activeTasks.values()) {
    task.cancelled = true;
    task.controller?.abort();
  }
  activeTasks.clear();
  queuedByLane.critical.length = 0;
  queuedByLane.visible.length = 0;
  queuedByLane.prefetch.length = 0;
  activeByLane.critical = 0;
  activeByLane.visible = 0;
  activeByLane.prefetch = 0;
  activeTotal = 0;
  activeHeavy = 0;
}

export type GridTraceStatus = 'ok' | 'near_threshold' | 'missed' | 'pause' | 'skipped' | 'aborted';
export type GridTraceCause = 'presentation_bound' | 'draw_bound' | 'pipeline_bound' | 'prep_bound' | 'stale_work' | 'idle_noise' | 'unknown';
export type GridTraceConsoleMode = 'none' | 'flagged' | 'all';
export type GridTraceCaptureScope = 'scroll_only' | 'all_frames';

export interface GridTraceEvent {
  atMs: number;
  type: string;
  payload?: Record<string, unknown>;
}

export interface GridTraceScrollSnapshot {
  active: boolean;
  phase: string;
  direction: string;
  velocityPxPerSec: number;
}

export interface GridTraceRafSnapshot {
  requestedAt: number | null;
  firedAt: number | null;
  delayMs: number;
  frameGapMs: number;
  hadPendingRaf: boolean;
  staleReset: boolean;
  reasons: string[];
}

export interface GridTraceVisibilitySnapshot {
  startIdx: number;
  endIdx: number;
  visibleIterEnd: number;
  prefetchCount: number;
  cancelTop: number;
  cancelBottom: number;
}

export interface GridTracePipelineSnapshot {
  queueDepth: number;
  activeLoads: number;
  cacheEntries: number;
  totalBytes: number;
  visibleThumbs: {
    unique: number;
    ready: number;
    loading: number;
    queued: number;
    missing: number;
  };
  ensureVisibleCount: number;
  ensurePrefetchCount: number;
  cancelCount: number;
  evictCount: number;
  staleWorkCount: number;
  visibleImpactCount: number;
}

export interface GridTraceDrawSnapshot {
  preconditionsMs: number;
  visibilityMs: number;
  pipelineMs: number;
  clearMs: number;
  imageDrawMs: number;
  chromeDrawMs: number;
  totalMs: number;
}

export interface GridTraceOutcome {
  firstPaint: boolean;
  activeReveal: boolean;
  scheduledNextFrame: boolean;
  dirtyBefore: {
    base: boolean;
    overlay: boolean;
  };
  dirtyAfter: {
    base: boolean;
    overlay: boolean;
  };
}

export interface GridFrameTrace {
  frameId: number;
  startedAt: number;
  endedAt: number;
  durationMs: number;
  budgetMs: number;
  status: GridTraceStatus;
  cause: GridTraceCause;
  scrollState: GridTraceScrollSnapshot;
  raf: GridTraceRafSnapshot;
  visibility: GridTraceVisibilitySnapshot;
  pipeline: GridTracePipelineSnapshot;
  draw: GridTraceDrawSnapshot;
  outcome: GridTraceOutcome;
  events: GridTraceEvent[];
}

export interface GridTraceCaptureSettings {
  enabled: boolean;
  consoleMode: GridTraceConsoleMode;
  captureScope: GridTraceCaptureScope;
  maxFrames: number;
  includeEventPayloads: boolean;
  includeSuccessfulFrames: boolean;
}

export interface GridFrameTraceStoreSnapshot {
  settings: GridTraceCaptureSettings;
  traces: GridFrameTrace[];
  lastFlaggedTrace: GridFrameTrace | null;
  updatedAt: number;
}

const TRACE_SETTINGS_KEY = 'grid-trace-settings';
const BUDGET_120HZ_MS = 8.33;
const MISSED_FRAME_MS = 10;
const PAUSE_FRAME_MS = 100;

const DEFAULT_SETTINGS: GridTraceCaptureSettings = {
  enabled: false,
  consoleMode: 'flagged',
  captureScope: 'all_frames',
  maxFrames: 300,
  includeEventPayloads: true,
  includeSuccessfulFrames: true,
};

type TraceSubscriber = (snapshot: GridFrameTraceStoreSnapshot) => void;

function cloneTrace(trace: GridFrameTrace): GridFrameTrace {
  return {
    ...trace,
    scrollState: { ...trace.scrollState },
    raf: { ...trace.raf, reasons: [...trace.raf.reasons] },
    visibility: { ...trace.visibility },
    pipeline: {
      ...trace.pipeline,
      visibleThumbs: { ...trace.pipeline.visibleThumbs },
    },
    draw: { ...trace.draw },
    outcome: {
      ...trace.outcome,
      dirtyBefore: { ...trace.outcome.dirtyBefore },
      dirtyAfter: { ...trace.outcome.dirtyAfter },
    },
    events: trace.events.map((event) => ({
      atMs: event.atMs,
      type: event.type,
      payload: event.payload ? { ...event.payload } : undefined,
    })),
  };
}

function loadSettings(): GridTraceCaptureSettings {
  if (typeof window === 'undefined') return DEFAULT_SETTINGS;
  try {
    const raw = window.localStorage.getItem(TRACE_SETTINGS_KEY);
    if (!raw) return DEFAULT_SETTINGS;
    const parsed = JSON.parse(raw) as Partial<GridTraceCaptureSettings>;
    return {
      ...DEFAULT_SETTINGS,
      ...parsed,
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

function saveSettings(settings: GridTraceCaptureSettings): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(TRACE_SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // Ignore localStorage failures in debug-only tooling.
  }
}

function isFlagged(trace: GridFrameTrace): boolean {
  return trace.status === 'missed'
    || trace.status === 'pause'
    || trace.draw.totalMs > trace.budgetMs
    || trace.pipeline.visibleImpactCount > 0;
}

function summarizeTrace(trace: GridFrameTrace): string {
  return `[grid-trace] frame=${trace.frameId} status=${trace.status} cause=${trace.cause} gap=${trace.raf.frameGapMs.toFixed(2)}ms raf=${trace.raf.delayMs.toFixed(2)}ms total=${trace.durationMs.toFixed(2)}ms draw=${trace.draw.totalMs.toFixed(2)}ms visible=${trace.visibility.visibleIterEnd} ready=${trace.pipeline.visibleThumbs.ready} queue=${trace.pipeline.queueDepth} loads=${trace.pipeline.activeLoads} stale=${trace.pipeline.staleWorkCount} impact=${trace.pipeline.visibleImpactCount}`;
}

class GridTraceRecorder {
  private settings = loadSettings();
  private traces: GridFrameTrace[] = [];
  private lastFlaggedTrace: GridFrameTrace | null = null;
  private nextFrameId = 1;
  private subscribers = new Set<TraceSubscriber>();

  constructor() {
    this.installWindowApi();
  }

  getSettings(): GridTraceCaptureSettings {
    return { ...this.settings };
  }

  setSettings(next: Partial<GridTraceCaptureSettings>): GridTraceCaptureSettings {
    this.settings = {
      ...this.settings,
      ...next,
    };
    saveSettings(this.settings);
    this.publish();
    return this.getSettings();
  }

  shouldCapture(scrollActive: boolean): boolean {
    if (!this.settings.enabled) return false;
    if (this.settings.captureScope === 'scroll_only' && !scrollActive) return false;
    return true;
  }

  allocateFrameId(): number {
    const frameId = this.nextFrameId;
    this.nextFrameId += 1;
    return frameId;
  }

  record(trace: GridFrameTrace): void {
    if (!this.settings.includeSuccessfulFrames && !isFlagged(trace)) {
      return;
    }

    this.traces.push(cloneTrace(trace));
    if (this.traces.length > this.settings.maxFrames) {
      this.traces.splice(0, this.traces.length - this.settings.maxFrames);
    }
    if (isFlagged(trace)) {
      this.lastFlaggedTrace = cloneTrace(trace);
    }

    if (this.settings.consoleMode === 'all' || (this.settings.consoleMode === 'flagged' && isFlagged(trace))) {
      console.warn(summarizeTrace(trace), cloneTrace(trace));
    }

    this.publish();
  }

  clear(): void {
    this.traces = [];
    this.lastFlaggedTrace = null;
    this.publish();
  }

  dump(): GridFrameTrace[] {
    return this.traces.map(cloneTrace);
  }

  getSnapshot(): GridFrameTraceStoreSnapshot {
    return {
      settings: this.getSettings(),
      traces: this.dump(),
      lastFlaggedTrace: this.lastFlaggedTrace ? cloneTrace(this.lastFlaggedTrace) : null,
      updatedAt: performance.now(),
    };
  }

  subscribe(fn: TraceSubscriber): () => void {
    this.subscribers.add(fn);
    return () => {
      this.subscribers.delete(fn);
    };
  }

  private publish(): void {
    const snapshot = this.getSnapshot();
    for (const subscriber of this.subscribers) {
      subscriber(snapshot);
    }
  }

  private installWindowApi(): void {
    if (typeof window === 'undefined') return;
    window.__gridTrace = {
      dump: () => this.dump(),
      clear: () => this.clear(),
      enable: () => this.setSettings({ enabled: true }),
      disable: () => this.setSettings({ enabled: false }),
      getSettings: () => this.getSettings(),
      setSettings: (next: Partial<GridTraceCaptureSettings>) => this.setSettings(next),
    };
  }
}

export const gridTraceRecorder = new GridTraceRecorder();

export function createTraceEvent(
  frameStartedAt: number,
  type: string,
  payload?: Record<string, unknown>,
  settings: GridTraceCaptureSettings = DEFAULT_SETTINGS,
): GridTraceEvent {
  return {
    atMs: Math.max(0, performance.now() - frameStartedAt),
    type,
    payload: settings.includeEventPayloads ? payload : undefined,
  };
}

export function classifyTraceStatus(frameGapMs: number, durationMs: number): GridTraceStatus {
  if (frameGapMs >= PAUSE_FRAME_MS) return 'pause';
  if (frameGapMs >= MISSED_FRAME_MS) return 'missed';
  if (frameGapMs > BUDGET_120HZ_MS || durationMs > BUDGET_120HZ_MS) return 'near_threshold';
  return 'ok';
}

export function inferTraceCause(trace: GridFrameTrace): GridTraceCause {
  if (trace.pipeline.visibleImpactCount > 0) return 'stale_work';
  if (trace.status === 'pause') return 'unknown';
  if (trace.draw.totalMs > trace.budgetMs || trace.draw.imageDrawMs + trace.draw.chromeDrawMs > trace.budgetMs * 0.75) {
    return 'draw_bound';
  }
  const pendingVisible = trace.pipeline.visibleThumbs.loading + trace.pipeline.visibleThumbs.queued + trace.pipeline.visibleThumbs.missing;
  if (pendingVisible > 0 && trace.pipeline.activeLoads > 0) {
    return 'pipeline_bound';
  }
  if (trace.draw.preconditionsMs + trace.draw.visibilityMs + trace.draw.pipelineMs > trace.budgetMs * 0.75) {
    return 'prep_bound';
  }
  if (
    trace.status === 'missed'
    && trace.draw.totalMs < trace.budgetMs * 0.5
    && pendingVisible === 0
  ) {
    return 'presentation_bound';
  }
  if (trace.scrollState.active && (trace.raf.delayMs > 0 || trace.raf.frameGapMs > trace.budgetMs)) {
    return 'presentation_bound';
  }
  if (trace.status === 'ok' && !trace.outcome.activeReveal && trace.visibility.visibleIterEnd === 0) {
    return 'idle_noise';
  }
  return 'unknown';
}

declare global {
  interface Window {
    __gridTrace?: {
      dump: () => GridFrameTrace[];
      clear: () => void;
      enable: () => GridTraceCaptureSettings;
      disable: () => GridTraceCaptureSettings;
      getSettings: () => GridTraceCaptureSettings;
      setSettings: (next: Partial<GridTraceCaptureSettings>) => GridTraceCaptureSettings;
    };
  }
}

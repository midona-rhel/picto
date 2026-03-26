/**
 * Frame profiler — per-phase timing for the canvas draw loop.
 *
 * Zero-allocation in the hot path: uses pre-allocated Float64Arrays for
 * timing storage. Tracks a rolling window of the last N frames with
 * per-phase breakdown.
 *
 * Phases (in draw order):
 *   0: visibilityPlan  — binary search + prefetch zone computation
 *   1: hashCollection  — building visible/ahead/behind request lists
 *   2: pipeline        — request + evict calls
 *   3: revealCompute   — reveal slot assignment + progress calculation
 *   4: clear           — background fill
 *   5: imageDraw       — placeholder and image drawing
 *   6: chromeDraw      — borders, badges, stars, and text
 *   6:
 *   7: total           — entire draw() function wall time
 *
 * Usage:
 *   const profiler = new FrameProfiler();
 *   profiler.begin();                    // start frame
 *   profiler.mark(Phase.visibilityPlan); // end of visibility plan
 *   profiler.mark(Phase.hashCollection); // end of hash collection
 *   ...
 *   profiler.end();                      // commit frame timings
 *   profiler.getStats();                 // rolling window stats
 */

const WINDOW_SIZE = 120; // ~1 second at 120Hz

export const Phase = {
  visibilityPlan: 0,
  hashCollection: 1,
  pipeline: 2,
  revealCompute: 3,
  clear: 4,
  imageDraw: 5,
  chromeDraw: 6,

  total: 7,
} as const;

export type PhaseId = (typeof Phase)[keyof typeof Phase];

export const PHASE_NAMES: Record<number, string> = {
  [Phase.visibilityPlan]: 'visibility',
  [Phase.hashCollection]: 'hashes',
  [Phase.pipeline]: 'pipeline',
  [Phase.revealCompute]: 'reveal',
  [Phase.clear]: 'clear',
  [Phase.imageDraw]: 'images',
  [Phase.chromeDraw]: 'chrome',

  [Phase.total]: 'TOTAL',
};

const PHASE_COUNT = 8;
const BUDGET_120HZ = 8.33; // ms
const MISSED_FRAME_MS = 10;
const PAUSE_FRAME_MS = 100;

export interface FrameStats {
  frameCount: number;
  /** Per-phase: { name, avg, max, p99 } in ms */
  phases: Array<{ name: string; avg: number; max: number; p99: number }>;
  /** Meaningful missed frames, excluding near-threshold variance and pauses */
  droppedFrames: number;
  /** Frame gaps over budget but below the missed-frame threshold */
  nearThresholdFrames: number;
  /** Frame gaps at or above MISSED_FRAME_MS and below PAUSE_FRAME_MS */
  missedFrames: number;
  /** Very long gaps typically caused by tab pauses, HMR, or devtools */
  pauseFrames: number;
  /** draw() wall times over budget */
  drawOverBudgetFrames: number;
  /** Current fps based on recent frame intervals */
  fps: number;
  avgFrameGapMs: number;
  maxFrameGapMs: number;
  maxMissedFrameGapMs: number;
  maxPauseGapMs: number;
  avgRafDelayMs: number;
  maxRafDelayMs: number;
}

export interface FrameCommitStats {
  frameGapMs: number;
  rafDelayMs: number;
  totalMs: number;
  drawOverBudget: boolean;
}

export class FrameProfiler {
  // Ring buffer: each frame is PHASE_COUNT floats (one per phase duration in ms)
  private buffer = new Float64Array(WINDOW_SIZE * PHASE_COUNT);
  private writeIndex = 0;
  private frameCount = 0;

  // Per-frame scratch: marks[phase] = timestamp
  private marks = new Float64Array(PHASE_COUNT);
  private frameStart = 0;

  // Frame interval tracking for FPS
  private lastFrameEnd = 0;
  private intervalBuffer = new Float64Array(WINDOW_SIZE);
  private rafDelayBuffer = new Float64Array(WINDOW_SIZE);
  private expectedCadenceBuffer = new Uint8Array(WINDOW_SIZE);
  private currentRafDelayMs = 0;

  private enabled = true;
  private warnOnDrop = true;

  /** Start a new frame measurement. Call at the very beginning of draw(). */
  begin() {
    if (!this.enabled) return;
    this.frameStart = performance.now();
    this.marks.fill(0);
  }

  noteRafDelay(delayMs: number) {
    if (!this.enabled) return;
    this.currentRafDelayMs = Math.max(0, delayMs);
  }

  /** Mark the end of a phase. Phases must be marked in order. */
  mark(phase: PhaseId) {
    if (!this.enabled) return;
    this.marks[phase] = performance.now();
  }

  /** End the frame and commit timings to the ring buffer. */
  end(context?: { visibleTiles?: number; expectContinuousFrames?: boolean }): FrameCommitStats | null {
    if (!this.enabled) return null;
    const now = performance.now();
    this.marks[Phase.total] = now;
    const rafDelayMs = this.currentRafDelayMs;

    // Compute per-phase durations
    const offset = (this.writeIndex % WINDOW_SIZE) * PHASE_COUNT;
    let prevTime = this.frameStart;

    for (let p = 0; p < PHASE_COUNT; p++) {
      const markTime = this.marks[p];
      if (markTime > 0) {
        this.buffer[offset + p] = markTime - prevTime;
        prevTime = markTime;
      } else {
        this.buffer[offset + p] = 0;
      }
    }
    // Total is wall time, not sum of phases (avoids rounding drift)
    this.buffer[offset + Phase.total] = now - this.frameStart;

    // Frame-gap tracking
    let frameGapMs = 0;
    if (this.lastFrameEnd > 0) {
      const interval = now - this.lastFrameEnd;
      frameGapMs = interval;
      this.intervalBuffer[this.writeIndex % WINDOW_SIZE] = interval;
    } else {
      this.intervalBuffer[this.writeIndex % WINDOW_SIZE] = 0;
    }
    this.rafDelayBuffer[this.writeIndex % WINDOW_SIZE] = this.currentRafDelayMs;
    this.expectedCadenceBuffer[this.writeIndex % WINDOW_SIZE] = context?.expectContinuousFrames ? 1 : 0;
    this.currentRafDelayMs = 0;
    this.lastFrameEnd = now;

    this.writeIndex++;
    this.frameCount++;

    // Warn only for meaningful misses, not normal near-threshold variance
    const totalMs = now - this.frameStart;
    if (this.warnOnDrop && frameGapMs >= PAUSE_FRAME_MS) {
      this.logPauseFrame(frameGapMs, rafDelayMs, totalMs);
    }
    if (this.warnOnDrop && totalMs > BUDGET_120HZ) {
      this.logDrawOverBudgetFrame(totalMs, context?.visibleTiles);
    }

    return {
      frameGapMs,
      rafDelayMs,
      totalMs,
      drawOverBudget: totalMs > BUDGET_120HZ,
    };
  }

  /** Get rolling window stats. Safe to call from dev overlay RAF. */
  getStats(): FrameStats {
    const n = Math.min(this.frameCount, WINDOW_SIZE);
    if (n === 0) {
      return {
        frameCount: 0,
        phases: Object.values(PHASE_NAMES).map((name) => ({ name, avg: 0, max: 0, p99: 0 })),
        droppedFrames: 0,
        nearThresholdFrames: 0,
        missedFrames: 0,
        pauseFrames: 0,
        drawOverBudgetFrames: 0,
        fps: 0,
        avgFrameGapMs: 0,
        maxFrameGapMs: 0,
        maxMissedFrameGapMs: 0,
        maxPauseGapMs: 0,
        avgRafDelayMs: 0,
        maxRafDelayMs: 0,
      };
    }

    // Collect per-phase values from the ring buffer
    const scratch = new Float64Array(n);
    const phases: FrameStats['phases'] = [];
    let droppedFrames = 0;
    let drawOverBudgetFrames = 0;
    let nearThresholdFrames = 0;
    let missedFrames = 0;
    let pauseFrames = 0;
    let gapSum = 0;
    let gapSamples = 0;
    let maxFrameGapMs = 0;
    let maxMissedFrameGapMs = 0;
    let maxPauseGapMs = 0;
    let rafDelaySum = 0;
    let rafDelaySamples = 0;
    let maxRafDelayMs = 0;

    for (let p = 0; p < PHASE_COUNT; p++) {
      const name = PHASE_NAMES[p];
      if (!name) continue;

      // Extract phase values from ring buffer
      for (let i = 0; i < n; i++) {
        const idx = ((this.writeIndex - n + i + WINDOW_SIZE) % WINDOW_SIZE) * PHASE_COUNT + p;
        scratch[i] = this.buffer[idx];
      }

      // Sort for percentile calculation
      const sorted = scratch.slice(0, n).sort();
      let sum = 0;
      for (let i = 0; i < n; i++) sum += sorted[i];

      phases.push({
        name,
        avg: sum / n,
        max: sorted[n - 1],
        p99: sorted[Math.floor(n * 0.99)],
      });

      // Count dropped frames from total phase
      if (p === Phase.total) {
        for (let i = 0; i < n; i++) {
          if (sorted[i] > BUDGET_120HZ) drawOverBudgetFrames++;
        }
      }
    }

    for (let i = 0; i < n; i++) {
      const idx = (this.writeIndex - n + i + WINDOW_SIZE) % WINDOW_SIZE;
      const gap = this.intervalBuffer[idx];
      if (gap <= 0) continue;
      if (this.expectedCadenceBuffer[idx] === 0) continue;
      if (gap > maxFrameGapMs) maxFrameGapMs = gap;
      if (gap >= PAUSE_FRAME_MS) {
        pauseFrames++;
        if (gap > maxPauseGapMs) maxPauseGapMs = gap;
      } else {
        gapSum += gap;
        gapSamples++;
        if (gap >= MISSED_FRAME_MS) {
          missedFrames++;
          if (gap > maxMissedFrameGapMs) maxMissedFrameGapMs = gap;
        } else if (gap > BUDGET_120HZ) {
          nearThresholdFrames++;
        }
      }
      const rafDelay = this.rafDelayBuffer[idx];
      rafDelaySum += rafDelay;
      rafDelaySamples++;
      if (rafDelay > maxRafDelayMs) maxRafDelayMs = rafDelay;
    }

    droppedFrames = missedFrames;
    const fps = gapSamples > 0 ? Math.round(1000 / (gapSum / gapSamples)) : 0;

    return {
      frameCount: this.frameCount,
      phases,
      droppedFrames,
      nearThresholdFrames,
      missedFrames,
      pauseFrames,
      drawOverBudgetFrames,
      fps,
      avgFrameGapMs: gapSamples > 0 ? gapSum / gapSamples : 0,
      maxFrameGapMs,
      maxMissedFrameGapMs,
      maxPauseGapMs,
      avgRafDelayMs: rafDelaySamples > 0 ? rafDelaySum / rafDelaySamples : 0,
      maxRafDelayMs,
    };
  }

  /** Format stats as a compact string for dev overlay or console. */
  formatStats(): string {
    const s = this.getStats();
    if (s.frameCount === 0) return 'No frames';

    const lines = [
      `${s.fps}fps | missed ${s.missedFrames}/${Math.min(s.frameCount, WINDOW_SIZE)} | draw ${s.drawOverBudgetFrames}/${Math.min(s.frameCount, WINDOW_SIZE)} | near ${s.nearThresholdFrames} | pause ${s.pauseFrames}`,
      `gap avg=${s.avgFrameGapMs.toFixed(2)}ms max=${s.maxFrameGapMs.toFixed(2)}ms missedMax=${s.maxMissedFrameGapMs.toFixed(2)}ms pauseMax=${s.maxPauseGapMs.toFixed(2)}ms | raf avg=${s.avgRafDelayMs.toFixed(2)}ms max=${s.maxRafDelayMs.toFixed(2)}ms`,
    ];
    for (const p of s.phases) {
      if (p.avg < 0.001 && p.max < 0.001) continue; // skip unused phases
      lines.push(`  ${p.name.padEnd(10)} avg=${p.avg.toFixed(2)}ms  max=${p.max.toFixed(2)}ms  p99=${p.p99.toFixed(2)}ms`);
    }
    return lines.join('\n');
  }

  setEnabled(v: boolean) { this.enabled = v; }
  setWarnOnDrop(v: boolean) { this.warnOnDrop = v; }

  private logDrawOverBudgetFrame(totalMs: number, visibleTiles?: number) {
    const offset = ((this.writeIndex - 1 + WINDOW_SIZE) % WINDOW_SIZE) * PHASE_COUNT;
    const breakdown: string[] = [];
    for (let p = 0; p < PHASE_COUNT; p++) {
      const name = PHASE_NAMES[p];
      const ms = this.buffer[offset + p];
      if (name && ms > 0.1) {
        breakdown.push(`${name}=${ms.toFixed(2)}ms`);
      }
    }
    console.warn(
      `[grid-perf] draw-over-budget total=${totalMs.toFixed(2)}ms budget=${BUDGET_120HZ.toFixed(2)}ms visibleTiles=${visibleTiles ?? 0} | ${breakdown.join(' ')}`,
    );
  }

  private logPauseFrame(frameGapMs: number, rafDelayMs: number, totalMs: number) {
    console.info(
      `[grid-perf] pause gap=${frameGapMs.toFixed(2)}ms rafDelay=${rafDelayMs.toFixed(2)}ms draw=${totalMs.toFixed(2)}ms`,
    );
  }
}

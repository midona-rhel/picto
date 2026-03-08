import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  enqueueMediaQosTask,
  getMediaQosStats,
  resetMediaQosSchedulerForTests,
} from '../mediaQosScheduler';

function nextTick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function occupyAllSchedulerSlots(release: Array<() => void>, started?: string[]): void {
  const enqueueBlocking = (lane: 'critical' | 'visible' | 'prefetch', label: string) => {
    enqueueMediaQosTask({
      lane,
      run: () => new Promise<void>((resolve) => {
        release.push(resolve);
        started?.push(label);
      }),
    });
  };

  for (let i = 0; i < 4; i++) enqueueBlocking('critical', `critical-${i}`);
  for (let i = 0; i < 3; i++) enqueueBlocking('visible', `visible-${i}`);
  enqueueBlocking('prefetch', 'prefetch-0');
}

describe('mediaQosScheduler', () => {
  beforeEach(() => {
    resetMediaQosSchedulerForTests();
  });

  afterEach(() => {
    resetMediaQosSchedulerForTests();
  });

  it('prioritizes critical lane over queued prefetch work', async () => {
    const releaseActive: Array<() => void> = [];
    occupyAllSchedulerSlots(releaseActive);

    const started: string[] = [];
    const releaseQueued: Record<string, () => void> = {};
    enqueueMediaQosTask({
      lane: 'prefetch',
      priority: 20,
      run: () => new Promise<void>((resolve) => {
        started.push('queued-prefetch');
        releaseQueued.prefetch = resolve;
      }),
    });
    enqueueMediaQosTask({
      lane: 'critical',
      priority: 0,
      run: () => new Promise<void>((resolve) => {
        started.push('queued-critical');
        releaseQueued.critical = resolve;
      }),
    });

    await nextTick();
    expect(getMediaQosStats().activeTotal).toBe(8);
    expect(started).toEqual([]);

    releaseActive[0]();
    await nextTick();

    expect(started).toContain('queued-critical');
    expect(started[0]).toBe('queued-critical');

    releaseQueued.critical?.();
    releaseQueued.prefetch?.();
    for (const release of releaseActive.slice(1)) release();
    await nextTick();
  });

  it('enforces global heavy codec cap', async () => {
    const release: Array<() => void> = [];
    const startedHeavy: number[] = [];

    for (let i = 0; i < 4; i++) {
      enqueueMediaQosTask({
        lane: 'visible',
        heavy: true,
        run: () => new Promise<void>((resolve) => {
          startedHeavy.push(i);
          release.push(resolve);
        }),
      });
    }

    await nextTick();
    expect(startedHeavy).toHaveLength(2);
    expect(getMediaQosStats().activeHeavy).toBe(2);

    release[0]();
    await nextTick();

    expect(startedHeavy).toHaveLength(3);

    for (const done of release.slice(1)) done();
    await nextTick();
  });

  it('cancels queued tasks before they start', async () => {
    const release: Array<() => void> = [];
    let cancelledStarted = false;
    occupyAllSchedulerSlots(release);

    const handle = enqueueMediaQosTask({
      lane: 'critical',
      run: () => {
        cancelledStarted = true;
      },
    });

    await nextTick();
    handle.cancel();
    release[0]();
    await nextTick();

    expect(cancelledStarted).toBe(false);

    for (const done of release.slice(1)) done();
    await nextTick();
  });

  it('upgrades queued task lane priority', async () => {
    const release: Array<() => void> = [];
    const started: string[] = [];
    occupyAllSchedulerSlots(release);

    enqueueMediaQosTask({
      lane: 'prefetch',
      priority: 10,
      run: () => {
        started.push('prefetch-a');
      },
    });

    const promoteMe = enqueueMediaQosTask({
      lane: 'prefetch',
      priority: 20,
      run: () => {
        started.push('promoted');
      },
    });

    await nextTick();
    promoteMe.upgrade('critical', 0);

    release[0]();
    await nextTick();

    expect(started[0]).toBe('promoted');

    for (const done of release.slice(1)) done();
    await nextTick();
  });
});

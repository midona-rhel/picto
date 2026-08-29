import { describe, expect, it, vi } from 'vitest';
import { createSubscriptionActionQueue } from './subscriptionActionQueue';

describe('subscription action queue', () => {
  it('settles a posts-per-run update before starting the run requested by the same click', async () => {
    const calls: string[] = [];
    let settlePostsPerRun = () => {};
    const enqueue = createSubscriptionActionQueue();

    const update = enqueue(() => new Promise<void>((resolve) => {
      calls.push('posts-per-run:start');
      settlePostsPerRun = () => {
        calls.push('posts-per-run:done');
        resolve();
      };
    }));
    const run = enqueue(async () => {
      calls.push('run');
    });

    await vi.waitFor(() => expect(calls).toEqual(['posts-per-run:start']));
    settlePostsPerRun();
    await Promise.all([update, run]);

    expect(calls).toEqual(['posts-per-run:start', 'posts-per-run:done', 'run']);
  });

  it('continues with the next action after a rejected action', async () => {
    const enqueue = createSubscriptionActionQueue();
    const failed = enqueue(() => Promise.reject(new Error('invalid state')));
    const next = enqueue(() => Promise.resolve('started'));

    await expect(failed).rejects.toThrow('invalid state');
    await expect(next).resolves.toBe('started');
  });
});

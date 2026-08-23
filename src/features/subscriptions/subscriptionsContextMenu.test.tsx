import { describe, expect, it, vi } from 'vitest';
import type { MenuItem } from '../../shared/ui/ContextMenu/ContextMenu';
import type { SubscriptionInfo } from '../../shared/types/subscriptions';
import { buildSubscriptionMenu } from './subscriptionsContextMenu';

const subscription = {
  id: '7',
  name: 'Artist',
  schedule: 'daily',
  paused: false,
  queries: [],
} as unknown as SubscriptionInfo;

function resetEntry(running: boolean, onReset = vi.fn()): [MenuItem, typeof onReset] {
  const entries = buildSubscriptionMenu({
    subscription,
    running,
    onRun: vi.fn(),
    onStop: vi.fn(),
    onPause: vi.fn(),
    onRename: vi.fn(),
    onSetSchedule: vi.fn(),
    onReset,
    onDelete: vi.fn(),
  });
  const entry = entries.find(
    (candidate): candidate is MenuItem =>
      'action' in candidate && candidate.label === 'Reset Sync History…',
  );
  if (!entry) throw new Error('Reset menu entry is missing');
  return [entry, onReset];
}

describe('subscription context menu', () => {
  it('offers reset when idle and invokes the reset operation', () => {
    const [entry, onReset] = resetEntry(false);

    expect(entry.disabled).toBe(false);
    entry.action?.();
    expect(onReset).toHaveBeenCalledOnce();
  });

  it('disables reset while the subscription is running', () => {
    const [entry] = resetEntry(true);

    expect(entry.disabled).toBe(true);
  });
});

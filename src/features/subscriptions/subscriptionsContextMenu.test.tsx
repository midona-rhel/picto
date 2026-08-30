import { describe, expect, it, vi } from 'vitest';
import { IconPlayerPlay, IconRefresh } from '@tabler/icons-react';
import type { ReactElement } from 'react';
import type { MenuItem } from '../../shared/ui/ContextMenu/ContextMenu';
import type { SubscriptionInfo } from '../../shared/types/subscriptions';
import { buildMultiCardMenu, buildSubscriptionMenu } from './subscriptionsContextMenu';

const subscription = {
  id: '7',
  name: 'Artist',
  schedule: 'daily',
  paused: false,
  queries: [],
} as unknown as SubscriptionInfo;

function resetEntry(
  running: boolean,
  onReset = vi.fn(),
  target = subscription,
): [MenuItem, typeof onReset] {
  const entries = buildSubscriptionMenu({
    subscription: target,
    active: running,
    running,
    onRun: vi.fn(),
    onStop: vi.fn(),
    onPauseRun: vi.fn(),
    onResumeRun: vi.fn(),
    onHold: vi.fn(),
    onRename: vi.fn(),
    onSetCover: vi.fn(),
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

  it('keeps reset disabled when a running subscription is paused', () => {
    const [pausedEntry] = resetEntry(true, vi.fn(), { ...subscription, paused: true });

    expect(pausedEntry.disabled).toBe(true);
  });

  it('uses play for resume and refresh for retry', () => {
    const build = (runStatus: string) => buildSubscriptionMenu({
      subscription: { ...subscription, run_status: runStatus },
      active: true,
      running: false,
      onRun: vi.fn(),
      onStop: vi.fn(),
      onPauseRun: vi.fn(),
      onResumeRun: vi.fn(),
      onHold: vi.fn(),
      onRename: vi.fn(),
      onSetCover: vi.fn(),
      onSetSchedule: vi.fn(),
      onReset: vi.fn(),
      onDelete: vi.fn(),
    })[0] as MenuItem;

    expect((build('paused').icon as ReactElement).type).toBe(IconPlayerPlay);
    expect((build('runtime').icon as ReactElement).type).toBe(IconRefresh);
  });

  it('applies a schedule to every selected subscription', () => {
    const onSetScheduleSelected = vi.fn();
    const entries = buildMultiCardMenu({
      subscriptionIds: ['1', '2'],
      schedules: ['daily', 'weekly'],
      anyActive: false,
      onRunSelected: vi.fn(),
      onPauseSelected: vi.fn(),
      onSetScheduleSelected,
      onDeleteSelected: vi.fn(),
    });
    const schedule = entries.find(
      (entry) => 'submenu' in entry && entry.submenu && entry.label === 'Schedule',
    );
    if (!schedule || !('children' in schedule)) throw new Error('Schedule submenu is missing');
    const monthly = schedule.children.find(
      (entry): entry is MenuItem => 'action' in entry && entry.label === 'Monthly',
    );
    monthly?.action?.();
    expect(onSetScheduleSelected).toHaveBeenCalledWith('monthly');
  });
});

/**
 * Context-menu builders for subscription cards and detail overflow menus all
 * build from here so actions stay consistent.
 *
 * Mirrors gridContextMenu.tsx: pure builders returning MenuEntry[] for the
 * shared ContextMenu primitive. Only destructive actions are `danger`.
 */

import {
  IconCheck,
  IconClock,
  IconPlayerPlay,
  IconPlayerPause,
  IconPlayerStop,
  IconPhotoEdit,
  IconRefresh,
  IconTrash,
} from '@tabler/icons-react';
import type { MenuEntry, MenuSeparator } from '../../shared/ui/ContextMenu/ContextMenu';
import { IconRename } from '../../shared/ui/icons/sidebar-menu-icons';
import type { SubscriptionInfo } from '../../shared/types/subscriptions';

function sep(): MenuSeparator {
  return { separator: true };
}

export interface SubscriptionMenuContext {
  subscription: SubscriptionInfo;
  running: boolean;
  onRun: () => void;
  onStop: () => void;
  onPause: (paused: boolean) => void;
  onRename: () => void;
  onSetCover: () => void;
  onSetSchedule: (schedule: string) => void;
  onReset: () => void;
  onDelete: () => void;
}

const SCHEDULES: Array<{ value: string; label: string }> = [
  { value: 'manual', label: 'Manual' },
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
];

export function buildSubscriptionMenu(ctx: SubscriptionMenuContext): MenuEntry[] {
  const { subscription, running } = ctx;

  return [
    running
      ? { label: 'Stop', icon: <IconPlayerStop size={14} />, action: ctx.onStop }
      : { label: 'Run Now', icon: <IconPlayerPlay size={14} />, action: ctx.onRun, disabled: subscription.paused },
    subscription.paused
      ? { label: 'Resume', icon: <IconPlayerPlay size={14} />, action: () => ctx.onPause(false) }
      : { label: 'Pause', icon: <IconPlayerPause size={14} />, action: () => ctx.onPause(true) },
    sep(),
    { label: 'Rename', icon: <IconRename />, action: ctx.onRename },
    { label: 'Set Cover Photo…', icon: <IconPhotoEdit size={14} />, action: ctx.onSetCover },
    {
      submenu: true,
      label: 'Schedule',
      icon: <IconClock size={14} />,
      children: SCHEDULES.map((entry): MenuEntry => ({
        label: entry.label,
        icon: subscription.schedule === entry.value ? <IconCheck size={14} /> : undefined,
        action: () => ctx.onSetSchedule(entry.value),
        disabled: subscription.schedule === entry.value,
      })),
    },
    sep(),
    {
      label: 'Reset Sync History…',
      icon: <IconRefresh size={14} />,
      action: ctx.onReset,
      disabled: running,
    },
    sep(),
    { label: 'Delete Subscription…', icon: <IconTrash size={14} />, action: ctx.onDelete, danger: true },
  ];
}

export interface MultiCardMenuContext {
  subscriptionIds: string[];
  anyRunning: boolean;
  onRunSelected: () => void;
  onPauseSelected: (paused: boolean) => void;
  onDeleteSelected: () => void;
}

export function buildMultiCardMenu(ctx: MultiCardMenuContext): MenuEntry[] {
  const total = ctx.subscriptionIds.length;
  return [
    {
      label: `Run ${total} Now`,
      icon: <IconPlayerPlay size={14} />,
      action: ctx.onRunSelected,
      disabled: ctx.anyRunning,
    },
    { label: `Pause ${total}`, icon: <IconPlayerPause size={14} />, action: () => ctx.onPauseSelected(true) },
    { label: `Resume ${total}`, icon: <IconPlayerPlay size={14} />, action: () => ctx.onPauseSelected(false) },
    sep(),
    { label: `Delete ${total}…`, icon: <IconTrash size={14} />, action: ctx.onDeleteSelected, danger: true },
  ];
}

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
  IconHandStop,
  IconPlayerPause,
  IconPlayerPlay,
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
  active: boolean;
  running: boolean;
  onRun: () => void;
  onStop: () => void;
  onPauseRun: () => void;
  onResumeRun: () => void;
  onHold: (held: boolean) => void;
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
  const { subscription, active, running } = ctx;
  const paused = subscription.run_status === 'paused';
  const retryLabel = paused ? 'Resume' : 'Retry';

  return [
    active
      ? running
        ? { label: 'Pause', icon: <IconPlayerPause size={14} />, action: ctx.onPauseRun }
        : { label: retryLabel, icon: paused ? <IconPlayerPlay size={14} /> : <IconRefresh size={14} />, action: ctx.onResumeRun }
      : { label: 'Run Now', icon: <IconPlayerPlay size={14} />, action: ctx.onRun, disabled: subscription.paused },
    active
      ? { label: 'Stop', icon: <IconPlayerStop size={14} />, action: ctx.onStop }
      : subscription.paused
        ? { label: 'Release Hold', icon: <IconPlayerPlay size={14} />, action: () => ctx.onHold(false) }
        : { label: 'Hold', icon: <IconHandStop size={14} />, action: () => ctx.onHold(true) },
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
      disabled: active,
    },
    sep(),
    { label: 'Delete Subscription…', icon: <IconTrash size={14} />, action: ctx.onDelete, danger: true },
  ];
}

export interface MultiCardMenuContext {
  subscriptionIds: string[];
  schedules: string[];
  anyActive: boolean;
  onRunSelected: () => void;
  onPauseSelected: (paused: boolean) => void;
  onSetScheduleSelected: (schedule: string) => void;
  onDeleteSelected: () => void;
}

export function buildMultiCardMenu(ctx: MultiCardMenuContext): MenuEntry[] {
  const total = ctx.subscriptionIds.length;
  const currentSchedule = new Set(ctx.schedules).size === 1 ? ctx.schedules[0] : null;
  return [
    {
      label: `Run ${total} Now`,
      icon: <IconPlayerPlay size={14} />,
      action: ctx.onRunSelected,
      disabled: ctx.anyActive,
    },
    { label: `Hold ${total}`, icon: <IconHandStop size={14} />, action: () => ctx.onPauseSelected(true), disabled: ctx.anyActive },
    { label: `Release Hold for ${total}`, icon: <IconPlayerPlay size={14} />, action: () => ctx.onPauseSelected(false), disabled: ctx.anyActive },
    {
      submenu: true,
      label: 'Schedule',
      icon: <IconClock size={14} />,
      children: SCHEDULES.map((entry): MenuEntry => ({
        label: entry.label,
        icon: currentSchedule === entry.value ? <IconCheck size={14} /> : undefined,
        action: () => ctx.onSetScheduleSelected(entry.value),
        disabled: currentSchedule === entry.value,
      })),
    },
    sep(),
    { label: `Delete ${total}…`, icon: <IconTrash size={14} />, action: ctx.onDeleteSelected, danger: true },
  ];
}

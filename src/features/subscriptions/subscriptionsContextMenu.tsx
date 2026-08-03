/**
 * Context-menu builders for the Following screen — cards, sidebar rows, and
 * detail overflow menus all build from here so actions stay consistent.
 *
 * Mirrors gridContextMenu.tsx: pure builders returning MenuEntry[] for the
 * shared ContextMenu primitive. Only destructive actions are `danger`.
 */

import {
  IconCheck,
  IconClock,
  IconFolderSymlink,
  IconPlayerPlay,
  IconPlayerPause,
  IconPlayerStop,
  IconRefresh,
  IconStack2,
  IconTrash,
} from '@tabler/icons-react';
import type { MenuEntry, MenuSeparator } from '../../shared/ui/ContextMenu/ContextMenu';
import { IconRename } from '../../shared/ui/icons/sidebar-menu-icons';
import type { SubscriptionGroupInfo, SubscriptionInfo } from '../../shared/types/subscriptions';

function sep(): MenuSeparator {
  return { separator: true };
}

export interface SubscriptionMenuContext {
  subscription: SubscriptionInfo;
  running: boolean;
  groups: SubscriptionGroupInfo[];
  onRun: () => void;
  onStop: () => void;
  onPause: (paused: boolean) => void;
  onRename: () => void;
  onMoveToGroup: (groupId: number | null) => void;
  onToggleAutoCollections: () => void;
  onReset: () => void;
  onDelete: () => void;
}

export function buildSubscriptionMenu(ctx: SubscriptionMenuContext): MenuEntry[] {
  const { subscription, running, groups } = ctx;
  const currentGroup = subscription.group_id ?? null;

  const groupChildren: MenuEntry[] = [
    {
      label: 'No group',
      icon: currentGroup == null ? <IconCheck size={14} /> : undefined,
      action: () => ctx.onMoveToGroup(null),
      disabled: currentGroup == null,
    },
    ...(groups.length > 0 ? [sep()] : []),
    ...groups.map((group): MenuEntry => ({
      label: group.name,
      icon: currentGroup === group.id ? <IconCheck size={14} /> : undefined,
      action: () => ctx.onMoveToGroup(Number(group.id)),
      disabled: currentGroup === group.id,
    })),
  ];

  return [
    running
      ? { label: 'Stop', icon: <IconPlayerStop size={14} />, action: ctx.onStop }
      : { label: 'Run Now', icon: <IconPlayerPlay size={14} />, action: ctx.onRun, disabled: subscription.paused },
    subscription.paused
      ? { label: 'Resume', icon: <IconPlayerPlay size={14} />, action: () => ctx.onPause(false) }
      : { label: 'Pause', icon: <IconPlayerPause size={14} />, action: () => ctx.onPause(true) },
    sep(),
    { label: 'Rename', icon: <IconRename />, action: ctx.onRename },
    { submenu: true, label: 'Move to Group', icon: <IconFolderSymlink size={14} />, children: groupChildren },
    {
      label: subscription.auto_collections ? 'Disable Post Collections' : 'Enable Post Collections',
      icon: <IconStack2 size={14} />,
      action: ctx.onToggleAutoCollections,
    },
    sep(),
    { label: 'Reset Sync Progress…', icon: <IconRefresh size={14} />, action: ctx.onReset },
    sep(),
    { label: 'Delete Subscription…', icon: <IconTrash size={14} />, action: ctx.onDelete, danger: true },
  ];
}

export interface GroupMenuContext {
  group: SubscriptionGroupInfo;
  anyRunning: boolean;
  onRunAll: () => void;
  onStopAll: () => void;
  onPauseGroup: (paused: boolean) => void;
  onRename: () => void;
  onSetSchedule: (schedule: string) => void;
  onDelete: () => void;
}

const SCHEDULES: Array<{ value: string; label: string }> = [
  { value: 'manual', label: 'Manual' },
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
];

export function buildGroupMenu(ctx: GroupMenuContext): MenuEntry[] {
  const { group, anyRunning } = ctx;
  return [
    anyRunning
      ? { label: 'Stop All', icon: <IconPlayerStop size={14} />, action: ctx.onStopAll }
      : { label: 'Run All Now', icon: <IconPlayerPlay size={14} />, action: ctx.onRunAll, disabled: group.paused },
    group.paused
      ? { label: 'Resume Group', icon: <IconPlayerPlay size={14} />, action: () => ctx.onPauseGroup(false) }
      : { label: 'Pause Group', icon: <IconPlayerPause size={14} />, action: () => ctx.onPauseGroup(true) },
    sep(),
    { label: 'Rename', icon: <IconRename />, action: ctx.onRename },
    {
      submenu: true,
      label: 'Schedule',
      icon: <IconClock size={14} />,
      children: SCHEDULES.map((entry): MenuEntry => ({
        label: entry.label,
        icon: group.schedule === entry.value ? <IconCheck size={14} /> : undefined,
        action: () => ctx.onSetSchedule(entry.value),
        disabled: group.schedule === entry.value,
      })),
    },
    sep(),
    { label: 'Delete Group…', icon: <IconTrash size={14} />, action: ctx.onDelete, danger: true },
  ];
}

export interface MultiCardMenuContext {
  subscriptionIds: string[];
  groupIds: string[];
  groups: SubscriptionGroupInfo[];
  onRunSelected: () => void;
  onPauseSelected: (paused: boolean) => void;
  onMoveSelectedToGroup: (groupId: number | null) => void;
  onDeleteSelected: () => void;
}

export function buildMultiCardMenu(ctx: MultiCardMenuContext): MenuEntry[] {
  const total = ctx.subscriptionIds.length + ctx.groupIds.length;
  const subsOnly = ctx.groupIds.length === 0;
  return [
    { label: `Run ${total} Now`, icon: <IconPlayerPlay size={14} />, action: ctx.onRunSelected },
    { label: `Pause ${total}`, icon: <IconPlayerPause size={14} />, action: () => ctx.onPauseSelected(true) },
    { label: `Resume ${total}`, icon: <IconPlayerPlay size={14} />, action: () => ctx.onPauseSelected(false) },
    sep(),
    ...(subsOnly
      ? [{
          submenu: true as const,
          label: `Move ${total} to Group`,
          icon: <IconFolderSymlink size={14} />,
          children: [
            { label: 'No group', action: () => ctx.onMoveSelectedToGroup(null) },
            ...(ctx.groups.length > 0 ? [sep()] : []),
            ...ctx.groups.map((group): MenuEntry => ({
              label: group.name,
              action: () => ctx.onMoveSelectedToGroup(Number(group.id)),
            })),
          ],
        }, sep()]
      : []),
    { label: `Delete ${total}…`, icon: <IconTrash size={14} />, action: ctx.onDeleteSelected, danger: true },
  ];
}

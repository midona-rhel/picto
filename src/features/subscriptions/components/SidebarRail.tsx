import { Fragment, useState } from 'react';
import {
  IconFolder,
  IconPlayerPlay,
  IconPlayerStop,
  IconPlus,
  IconShieldLock,
} from '@tabler/icons-react';
import type {
  SubscriptionGroupInfo,
  SubscriptionInfo,
  SubscriptionProgressEvent,
} from '../../../shared/types/subscriptions';
import type { SubscriptionListMetrics } from '../../../shared/types/subscriptionsWorkspace';
import type { SubscriptionsSelection } from '../../../state/subscriptionsWorkspace';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip/KbdTooltip';
import { SidebarRow } from '../../../shared/ui/SidebarRow/SidebarRow';
import { ActionButton } from './ActionButton';
import { describeSubscriptionState } from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

const SCHEDULE_LABELS: Record<string, string> = {
  daily: 'daily',
  weekly: 'weekly',
  monthly: 'monthly',
};

function statusDotClass(state: 'running' | 'paused' | 'attention' | 'idle'): string {
  return state === 'running'
    ? `${styles.railDot} ${styles.railDotRunning}`
    : state === 'paused'
      ? `${styles.railDot} ${styles.railDotPaused}`
      : state === 'attention'
        ? `${styles.railDot} ${styles.railDotAttention}`
        : styles.railDot;
}

/**
 * Left rail — the app's canonical sidebar tree (SidebarRow) with groups as
 * expandable folders and subscriptions as children. Hover reveals group
 * run/stop; footer hosts the New subscription dialog and Accounts.
 */
export function SidebarRail({
  groups,
  subscriptions,
  listMetrics,
  progressBySubscriptionId,
  runningSubscriptionIds,
  selection,
  busy,
  onSelect,
  onRunGroup,
  onStopGroup,
  onOpenWizard,
  onOpenAccounts,
  onCreateGroup,
}: {
  groups: SubscriptionGroupInfo[];
  subscriptions: SubscriptionInfo[];
  listMetrics: Record<string, SubscriptionListMetrics>;
  progressBySubscriptionId: Map<string, SubscriptionProgressEvent>;
  runningSubscriptionIds: string[];
  selection: SubscriptionsSelection;
  busy: boolean;
  onSelect: (selection: SubscriptionsSelection) => void;
  onRunGroup: (id: string) => void;
  onStopGroup: (id: string) => void;
  onOpenWizard: () => void;
  onOpenAccounts: () => void;
  onCreateGroup: () => void;
}) {
  // Groups start collapsed — expandedGroups only holds explicit opens.
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());
  const [sectionExpanded, setSectionExpanded] = useState(true);
  const grouped = new Set(groups.flatMap((group) => group.subscriptions.map((sub) => sub.id)));
  const ungrouped = subscriptions.filter((sub) => !grouped.has(sub.id));

  const toggleGroup = (groupId: string) => {
    setExpandedGroups((current) => {
      const next = new Set(current);
      if (next.has(groupId)) next.delete(groupId);
      else next.add(groupId);
      return next;
    });
  };

  const subscriptionRow = (subscription: SubscriptionInfo, indent: number, isLastChild: boolean) => {
    const metrics = listMetrics[subscription.id];
    const state = describeSubscriptionState({
      paused: subscription.paused,
      progress: progressBySubscriptionId.get(subscription.id) ?? null,
      failedPostCount: metrics?.failedPostCount ?? 0,
      openIssueCount: metrics?.openIssueCount ?? 0,
    });
    const attention = (metrics?.failedPostCount ?? 0) + (metrics?.openIssueCount ?? 0);
    return (
      <SidebarRow
        key={subscription.id}
        variant="folder"
        icon={<span className={statusDotClass(state)} />}
        label={subscription.name}
        count={attention > 0 ? attention : null}
        active={selection?.kind === 'subscription' && selection.id === subscription.id}
        indent={indent}
        isLastChild={isLastChild}
        treeLines={indent > 0 ? [true] : undefined}
        onClick={() => onSelect({ kind: 'subscription', id: subscription.id })}
      />
    );
  };

  return (
    <aside className={styles.sidebar}>
      <div className={styles.sidebarBody}>
        <SidebarRow
          variant="section"
          label="Subscriptions"
          expanded={sectionExpanded}
          onToggle={() => setSectionExpanded((v) => !v)}
          onAdd={onCreateGroup}
          addTooltip="New group"
        />
        {sectionExpanded && (
          <>
            {groups.map((group) => {
              const expanded = expandedGroups.has(group.id);
              const groupRunning = group.subscriptions.some((sub) =>
                runningSubscriptionIds.includes(sub.id));
              return (
                <Fragment key={group.id}>
                  <SidebarRow
                    variant="folder"
                    icon={<IconFolder size={15} />}
                    active={selection?.kind === 'group' && selection.id === group.id}
                    hasChildren={group.subscriptions.length > 0}
                    expanded={expanded}
                    onToggleExpand={() => toggleGroup(group.id)}
                    onClick={() => {
                      // Row click navigates only — the chevron owns expansion.
                      onSelect({ kind: 'group', id: group.id });
                    }}
                  >
                    <span className={styles.railRowName}>{group.name}</span>
                    {SCHEDULE_LABELS[group.schedule] && (
                      <span className={styles.railScheduleChip}>{SCHEDULE_LABELS[group.schedule]}</span>
                    )}
                    <span className={styles.railHoverActions}>
                      <KbdTooltip label={groupRunning ? 'Stop group' : 'Run group'}>
                        <button
                          type="button"
                          className={styles.querySmallBtn}
                          onClick={(e) => {
                            e.stopPropagation();
                            if (busy) return;
                            if (groupRunning) onStopGroup(group.id);
                            else onRunGroup(group.id);
                          }}
                        >
                          {groupRunning ? <IconPlayerStop size={12} /> : <IconPlayerPlay size={12} />}
                        </button>
                      </KbdTooltip>
                    </span>
                  </SidebarRow>
                  {expanded &&
                    group.subscriptions.map((subscription, index) =>
                      subscriptionRow(subscription, 1, index === group.subscriptions.length - 1))}
                </Fragment>
              );
            })}
            {ungrouped.map((subscription) => subscriptionRow(subscription, 0, true))}
            {groups.length === 0 && ungrouped.length === 0 && (
              <div className={styles.muted} role="note">
                <span className={styles.helper}>Nothing followed yet.</span>
              </div>
            )}
          </>
        )}
      </div>
      <div className={styles.railFooter}>
        <ActionButton variant="primary" onClick={onOpenWizard}>
          <IconPlus size={14} /> New subscription…
        </ActionButton>
        <ActionButton variant="ghost" onClick={onOpenAccounts}>
          <IconShieldLock size={14} /> Accounts
        </ActionButton>
      </div>
    </aside>
  );
}

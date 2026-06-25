import { useEffect, useRef, useState } from 'react';
import { IconDotsVertical, IconPlayerPlay, IconPlayerStop, IconX } from '@tabler/icons-react';
import type { SubscriptionGroupInfo, SubscriptionInfo } from '../../../shared/types/subscriptions';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip/KbdTooltip';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { ActionButton } from './ActionButton';
import { EmptyState } from './EmptyState';
import styles from '../SubscriptionsScreen.module.css';

const SCHEDULE_OPTIONS = [
  { value: 'manual', label: 'Manual' },
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
];

function GroupOverflowMenu({ busy, onDelete }: { busy: boolean; onDelete: () => void }) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener('mousedown', handler);
    return () => window.removeEventListener('mousedown', handler);
  }, [open]);

  return (
    <div className={styles.menuWrap} ref={wrapRef}>
      <button
        type="button"
        className={styles.querySmallBtn}
        aria-label="More actions"
        onClick={() => setOpen((v) => !v)}
      >
        <IconDotsVertical size={16} />
      </button>
      {open && (
        <div className={styles.menuPopover}>
          <button
            type="button"
            className={`${styles.menuItem} ${styles.menuItemDanger}`}
            disabled={busy}
            onClick={() => {
              setOpen(false);
              onDelete();
            }}
          >
            Delete group…
          </button>
        </div>
      )}
    </div>
  );
}

export function GroupDetail({
  group,
  allSubscriptions,
  runningSubscriptionIds,
  busy,
  onRename,
  onSetSchedule,
  onRun,
  onStop,
  onDelete,
  onAddSubscription,
  onRemoveSubscription,
  onSelectSubscription,
}: {
  group: SubscriptionGroupInfo;
  allSubscriptions: SubscriptionInfo[];
  runningSubscriptionIds: string[];
  busy: boolean;
  onRename: (name: string) => void;
  onSetSchedule: (schedule: string) => void;
  onRun: () => void;
  onStop: () => void;
  onDelete: () => void;
  onAddSubscription: (subscriptionId: string) => void;
  onRemoveSubscription: (subscriptionId: string) => void;
  onSelectSubscription: (subscriptionId: string) => void;
}) {
  const [name, setName] = useState(group.name);
  useEffect(() => setName(group.name), [group.id, group.name]);
  const memberIds = new Set(group.subscriptions.map((sub) => sub.id));
  const addable = allSubscriptions.filter((sub) => !memberIds.has(sub.id));
  const running = group.subscriptions.some((sub) => runningSubscriptionIds.includes(sub.id));
  const [addPick, setAddPick] = useState('');

  return (
    <div className={styles.content}>
      <div className={styles.hero}>
        <div className={styles.heroTop}>
          <div className={styles.titleWrap}>
            <input
              className={styles.titleInput}
              value={name}
              spellCheck={false}
              aria-label="Group name"
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                if (e.key === 'Escape') {
                  setName(group.name);
                  (e.target as HTMLInputElement).blur();
                }
              }}
              onBlur={() => {
                const trimmed = name.trim();
                if (trimmed && trimmed !== group.name) onRename(trimmed);
                else setName(group.name);
              }}
            />
            <span className={styles.heroMeta}>
              <span className={styles.muted}>
                {group.subscriptions.length} subscription{group.subscriptions.length === 1 ? '' : 's'} ·{' '}
                {group.total_files.toLocaleString()} files
              </span>
            </span>
          </div>
          <div className={styles.heroActions}>
            <span className={styles.fieldInline}>
              Schedule
              <CmSelect
                value={group.schedule}
                options={SCHEDULE_OPTIONS}
                onChange={onSetSchedule}
                width={100}
              />
            </span>
            {running ? (
              <ActionButton variant="secondary" disabled={busy} onClick={onStop}>
                <IconPlayerStop size={14} /> Stop all
              </ActionButton>
            ) : (
              <ActionButton variant="primary" disabled={busy || group.subscriptions.length === 0} onClick={onRun}>
                <IconPlayerPlay size={14} /> Run all
              </ActionButton>
            )}
            <GroupOverflowMenu busy={busy} onDelete={onDelete} />
          </div>
        </div>
      </div>

      <div className={styles.section}>
        <div className={styles.sectionHeader}>
          <span className={styles.sectionTitle}>Members</span>
        </div>
        {group.subscriptions.length === 0 ? (
          <EmptyState
            title="No subscriptions in this group"
            description="Add subscriptions below — when the group runs (or its schedule fires), all of them run."
          />
        ) : (
          group.subscriptions.map((subscription) => {
            const memberRunning = runningSubscriptionIds.includes(subscription.id);
            return (
              <div key={subscription.id} className={styles.memberRow}>
                <span
                  className={`${styles.railDot} ${
                    memberRunning ? styles.railDotRunning : subscription.paused ? styles.railDotPaused : ''
                  }`.trim()}
                />
                <button
                  type="button"
                  className={styles.linkButton}
                  onClick={() => onSelectSubscription(subscription.id)}
                >
                  {subscription.name}
                </button>
                <span className={styles.muted}>
                  {subscription.queries.length} quer{subscription.queries.length === 1 ? 'y' : 'ies'}
                </span>
                <span className={styles.muted}>{subscription.total_files.toLocaleString()} files</span>
                <span className={styles.queryCardActions}>
                  <KbdTooltip label="Remove from group">
                    <button
                      type="button"
                      className={styles.querySmallBtn}
                      disabled={busy}
                      onClick={() => onRemoveSubscription(subscription.id)}
                    >
                      <IconX size={14} />
                    </button>
                  </KbdTooltip>
                </span>
              </div>
            );
          })
        )}
        {addable.length > 0 && (
          <div className={styles.queryCardAdd}>
            <CmSelect
              value={addPick}
              options={[
                { value: '', label: 'Add subscription…' },
                ...addable.map((sub) => ({ value: sub.id, label: sub.name })),
              ]}
              onChange={setAddPick}
              width={220}
            />
            <ActionButton
              variant="secondary"
              disabled={busy || !addPick}
              onClick={() => {
                if (addPick) {
                  onAddSubscription(addPick);
                  setAddPick('');
                }
              }}
            >
              Add
            </ActionButton>
          </div>
        )}
      </div>
    </div>
  );
}

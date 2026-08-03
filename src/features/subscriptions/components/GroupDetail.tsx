import { useEffect, useState } from 'react';
import { IconPlayerPlay, IconPlayerStop, IconTrash, IconX } from '@tabler/icons-react';
import type { SubscriptionGroupInfo, SubscriptionSiteInfo } from '../../../shared/types/subscriptions';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip/KbdTooltip';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { mediaThumbnailUrl } from '../../../shared/lib/mediaUrl';
import { ActionButton } from './ActionButton';
import { AddQueryBar } from './AddQueryBar';
import styles from '../SubscriptionsScreen.module.css';

const SCHEDULE_OPTIONS = [
  { value: 'manual', label: 'Manual' },
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
];


export function GroupDetail({
  group,
  sites,
  runningSubscriptionIds,
  coverHash = null,
  busy,
  onRename,
  onSetSchedule,
  onRun,
  onStop,
  onDelete,
  onAddSource,
  onRemoveSubscription,
  onSelectSubscription,
}: {
  group: SubscriptionGroupInfo;
  sites: SubscriptionSiteInfo[];
  runningSubscriptionIds: string[];
  /** Newest downloaded file — hero image; null falls back to an initial. */
  coverHash?: string | null;
  busy: boolean;
  onRename: (name: string) => void;
  onSetSchedule: (schedule: string) => void;
  onRun: () => void;
  onStop: () => void;
  onDelete: () => void;
  onAddSource: (siteId: string, queryText: string) => Promise<void>;
  onRemoveSubscription: (subscriptionId: string) => void;
  onSelectSubscription: (subscriptionId: string) => void;
}) {
  const [name, setName] = useState(group.name);
  useEffect(() => setName(group.name), [group.id, group.name]);
  const running = group.subscriptions.some((sub) => runningSubscriptionIds.includes(sub.id));

  return (
    <div className={styles.content}>
      <div className={styles.hero}>
        <div className={styles.heroTop}>
          <div className={styles.heroIdentity}>
            <span className={styles.heroCover}>
              {coverHash ? (
                <img src={mediaThumbnailUrl(coverHash)} alt="" draggable={false} />
              ) : (
                <span className={styles.heroCoverFallback} aria-hidden>
                  {group.name.slice(0, 1).toUpperCase()}
                </span>
              )}
            </span>
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
                {group.subscriptions.length} source{group.subscriptions.length === 1 ? '' : 's'} ·{' '}
                {group.total_files.toLocaleString()} files
              </span>
            </span>
          </div>
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
            <KbdTooltip label="Delete group…">
              <button
                type="button"
                className={styles.querySmallBtn}
                aria-label="Delete group"
                disabled={busy}
                onClick={onDelete}
              >
                <IconTrash size={16} />
              </button>
            </KbdTooltip>
          </div>
        </div>
      </div>

      <div className={styles.section}>
        <div className={styles.sectionHeader}>
          <span className={styles.sectionTitle}>Sources</span>
        </div>
        {group.subscriptions.length === 0 ? (
          <div className={styles.sectionEmptyLine}>
            No sources yet — add a site below. When this runs (or its schedule fires), every
            source is checked.
          </div>
        ) : (
          <div className={styles.memberTable}>
            <div className={`${styles.memberTableRow} ${styles.qHeader}`}>
              <span>Source</span>
              <span className={styles.qCellNum}>Queries</span>
              <span className={styles.qCellNum}>Files</span>
              <span>Status</span>
              <span />
            </div>
            {group.subscriptions.map((subscription) => {
              const memberRunning = runningSubscriptionIds.includes(subscription.id);
              return (
                <div key={subscription.id} className={styles.memberTableRow}>
                  <span className={styles.qCellName}>
                    <button
                      type="button"
                      className={styles.linkButton}
                      onClick={() => onSelectSubscription(subscription.id)}
                    >
                      {subscription.name}
                    </button>
                  </span>
                  <span className={styles.qCellNum}>{subscription.queries.length}</span>
                  <span className={styles.qCellNum}>{subscription.total_files.toLocaleString()}</span>
                  <span className={styles.qCellStatus}>
                    <span
                      className={`${styles.qDot} ${
                        memberRunning
                          ? styles.qDotRunning
                          : subscription.paused
                            ? styles.qDotPaused
                            : styles.qDotIdle
                      }`.trim()}
                    />
                    {memberRunning ? 'Running' : subscription.paused ? 'Paused' : 'Idle'}
                  </span>
                  <span className={styles.qCellActions}>
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
            })}
          </div>
        )}
        <AddQueryBar
          sites={sites}
          busy={busy}
          onAdd={onAddSource}
        />
      </div>
    </div>
  );
}

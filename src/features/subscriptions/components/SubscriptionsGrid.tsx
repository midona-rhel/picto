import { useEffect, useRef, useState } from 'react';
import { IconDownload, IconLibraryPlus, IconPlus, IconShieldLock } from '@tabler/icons-react';
import type {
  SubscriptionCover,
  SubscriptionInfo,
  SubscriptionProgressEvent,
} from '../../../shared/types/subscriptions';
import type { SubscriptionListMetrics } from '../../../shared/types/subscriptionsWorkspace';
import type { SubscriptionsSelection } from '../../../state/subscriptionsWorkspace';
import { SubscriptionCoverDisplay } from './SubscriptionCoverImage';
import { isSubscriptionCompleted } from '../subscriptionUtils';
import { ActionButton } from './ActionButton';
import { StatusBadge } from './StatusBadge';
import { ProgressBar } from '../../../shared/ui/ProgressBar/ProgressBar';
import { useShortcutScope } from '../../../shared/hooks/useShortcutScope';
import styles from '../SubscriptionsScreen.module.css';

interface CardModel {
  selection: SubscriptionsSelection;
  key: string;
  name: string;
  items: number;
  sources: number;
  cover: SubscriptionCover | null;
  running: boolean;
  paused: boolean;
  attention: boolean;
  completed: boolean;
  waitingForInbox: boolean;
}

function FollowCard({
  card,
  selected,
  onClick,
  onOpen,
  onContextMenu,
}: {
  card: CardModel;
  selected: boolean;
  onClick: (e: React.MouseEvent) => void;
  onOpen: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
}) {
  const [coverFailed, setCoverFailed] = useState(false);
  const showCover = card.cover != null && !coverFailed;

  useEffect(() => setCoverFailed(false), [card.cover?.file_hash]);
  const status = card.waitingForInbox
    ? { tone: 'paused' as const, label: 'Inbox full' }
    : card.paused
      ? { tone: 'paused' as const, label: 'Paused' }
      : card.running
        ? { tone: 'running' as const, label: 'Syncing' }
        : card.attention
          ? { tone: 'attention' as const, label: 'Warning' }
        : card.completed
          ? { tone: 'success' as const, label: 'Complete' }
          : { tone: 'idle' as const, label: 'Idle' };

  return (
    <button
      type="button"
      className={`${styles.followCard} ${selected ? styles.followCardSelected : ''}`.trim()}
      onClick={onClick}
      onDoubleClick={onOpen}
      onContextMenu={onContextMenu}
      onKeyDown={(e) => {
        if (e.key === 'Enter') onOpen();
      }}
    >
      <span className={styles.followCover}>
        {showCover ? (
          <SubscriptionCoverDisplay
            fileHash={card.cover?.file_hash as string}
            crop={{
              focusX: card.cover?.focus_x ?? 500,
              focusY: card.cover?.focus_y ?? 500,
              zoomPercent: card.cover?.zoom_percent ?? 100,
            }}
            alt=""
            loading="lazy"
            draggable={false}
            onError={() => setCoverFailed(true)}
          />
        ) : (
          <span className={styles.followCoverFallback} aria-hidden>
            <IconDownload size={28} stroke={1.35} />
          </span>
        )}
      </span>
      <span className={styles.followName}>
        <span className={styles.followNameText}>{card.name}</span>
        <StatusBadge tone={status.tone} label={status.label} />
      </span>
      <span className={styles.followMeta}>
        {card.items.toLocaleString()} item{card.items === 1 ? '' : 's'}
        {card.sources > 0 && ` · ${card.sources} source${card.sources === 1 ? '' : 's'}`}
      </span>
    </button>
  );
}

/**
 * Subscription home — a card grid of subscriptions. Covers come from the
 * newest downloaded file.
 *
 * Interaction matches the main grid: click selects, double-click (or Enter)
 * opens, Cmd/Ctrl toggles, Shift range-selects, right-click opens the context
 * menu for the clicked card (or the whole selection when it's part of one).
 */
export function SubscriptionsGrid({
  subscriptions,
  galleryJobs,
  listMetrics,
  covers,
  progressBySubscriptionId,
  runningSubscriptionIds,
  onSelect,
  onAdd,
  galleryImportRunning,
  onAddGallery,
  onOpenAccounts,
  onSubscriptionMenu,
  onMultiMenu,
}: {
  subscriptions: SubscriptionInfo[];
  galleryJobs: SubscriptionInfo[];
  listMetrics: Record<string, SubscriptionListMetrics>;
  covers: Map<string, SubscriptionCover>;
  progressBySubscriptionId: Map<string, SubscriptionProgressEvent>;
  runningSubscriptionIds: string[];
  onSelect: (selection: SubscriptionsSelection) => void;
  onAdd: () => void;
  /** Only one gallery download runs at a time — the button locks while one is active. */
  galleryImportRunning: boolean;
  onAddGallery: () => void;
  onOpenAccounts: () => void;
  onSubscriptionMenu: (position: { x: number; y: number }, id: string) => void;
  onMultiMenu: (position: { x: number; y: number }, subscriptionIds: string[]) => void;
}) {
  const running = new Set(runningSubscriptionIds);
  const hasAttention = (id: string) =>
    ((listMetrics[id]?.failedPostCount ?? 0) + (listMetrics[id]?.openIssueCount ?? 0)) > 0;

  const cards: CardModel[] = [
    ...subscriptions
      .map((sub): CardModel => ({
        selection: { kind: 'subscription', id: sub.id },
        key: `sub:${sub.id}`,
        name: sub.name,
        items: sub.total_items,
        sources: sub.queries.length,
        cover: covers.get(sub.id) ?? null,
        running: running.has(sub.id) || progressBySubscriptionId.has(sub.id),
        paused: sub.paused,
        attention: hasAttention(sub.id),
        completed: !running.has(sub.id)
          && !progressBySubscriptionId.has(sub.id)
          && isSubscriptionCompleted(sub),
        waitingForInbox: sub.run_status === 'inbox_full',
      })),
  ].sort((left, right) => (
    left.name.localeCompare(right.name, undefined, { sensitivity: 'base', numeric: true })
    || left.key.localeCompare(right.key)
  ));

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const anchorIndexRef = useRef<number | null>(null);

  // Drop selections for cards that no longer exist.
  const validKeys = new Set(cards.map((card) => card.key));
  const liveSelected = new Set([...selected].filter((key) => validKeys.has(key)));

  useShortcutScope((event) => {
    if (event.key !== 'Escape' || selected.size === 0) return;
    setSelected(new Set());
    return true;
  }, { priority: 20 });

  const handleCardClick = (index: number, card: CardModel, e: React.MouseEvent) => {
    e.preventDefault();
    if (e.shiftKey && anchorIndexRef.current != null) {
      const [lo, hi] = [Math.min(anchorIndexRef.current, index), Math.max(anchorIndexRef.current, index)];
      const range = cards.slice(lo, hi + 1).map((entry) => entry.key);
      setSelected((current) =>
        e.metaKey || e.ctrlKey ? new Set([...current, ...range]) : new Set(range));
      return;
    }
    if (e.metaKey || e.ctrlKey) {
      setSelected((current) => {
        const next = new Set(current);
        if (next.has(card.key)) next.delete(card.key);
        else next.add(card.key);
        return next;
      });
      anchorIndexRef.current = index;
      return;
    }
    setSelected(new Set([card.key]));
    anchorIndexRef.current = index;
  };

  const handleCardContextMenu = (index: number, card: CardModel, e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    let menuSelection = liveSelected;
    if (!liveSelected.has(card.key)) {
      menuSelection = new Set([card.key]);
      setSelected(menuSelection);
      anchorIndexRef.current = index;
    }
    const position = { x: e.clientX, y: e.clientY };
    if (menuSelection.size > 1) {
      onMultiMenu(position, [...menuSelection].map((key) => key.slice('sub:'.length)));
      return;
    }
    if (card.selection?.kind === 'subscription') onSubscriptionMenu(position, card.selection.id);
  };

  return (
    <div className={styles.followingRoot}>
      <div className={styles.followingHeader}>
        <div className={styles.titleWrap}>
          <span className={styles.heroTitle}>Subscriptions</span>
          <span className={styles.muted}>
            {cards.length === 0
              ? 'No subscriptions yet'
              : liveSelected.size > 1
                ? `${liveSelected.size} of ${cards.length} selected`
                : `${cards.length} subscription${cards.length === 1 ? '' : 's'}`}
          </span>
        </div>
        <section
          className={styles.galleryJobs}
          data-active={galleryJobs.length > 0 || undefined}
          aria-label="Gallery downloads"
        >
          {galleryJobs.map((job) => {
            const progress = progressBySubscriptionId.get(job.id);
            const downloaded = progress?.files_downloaded ?? 0;
            const total = progress?.gallery_total_items ?? null;
            return (
              <div className={styles.galleryJob} key={job.id}>
                <IconDownload size={15} stroke={1.6} aria-hidden />
                <div className={styles.galleryJobBody}>
                  <div className={styles.galleryJobText}>
                    <span>{job.name}</span>
                    <span>
                      {total != null
                        ? `${downloaded.toLocaleString()} / ${total.toLocaleString()} images downloaded`
                        : `${downloaded.toLocaleString()} images downloaded`}
                    </span>
                  </div>
                  <ProgressBar
                    done={downloaded}
                    total={total ?? 0}
                    indeterminate={total == null}
                    height={2}
                  />
                </div>
              </div>
            );
          })}
        </section>
        <div className={styles.heroActions}>
          <ActionButton variant="primary" onClick={onAdd}>
            <IconPlus size={14} /> Add
          </ActionButton>
          <ActionButton variant="secondary" disabled={galleryImportRunning} onClick={onAddGallery}>
            <IconLibraryPlus size={14} /> Add Gallery
          </ActionButton>
          <ActionButton variant="ghost" onClick={onOpenAccounts}>
            <IconShieldLock size={14} /> Accounts
          </ActionButton>
        </div>
      </div>

      {cards.length === 0 ? (
        <div className={styles.sectionEmptyLine}>
          Create a subscription, then add the artists, tags, or accounts it should follow.
        </div>
      ) : (
        <div
          className={styles.followingGrid}
          onClick={(e) => {
            if (e.target === e.currentTarget) setSelected(new Set());
          }}
        >
          {cards.map((card, index) => (
            <FollowCard
              key={card.key}
              card={card}
              selected={liveSelected.has(card.key)}
              onClick={(e) => handleCardClick(index, card, e)}
              onOpen={() => onSelect(card.selection)}
              onContextMenu={(e) => handleCardContextMenu(index, card, e)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

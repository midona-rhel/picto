import { useEffect, useRef, useState } from 'react';
import { IconPlus, IconShieldLock } from '@tabler/icons-react';
import type {
  SubscriptionCover,
  SubscriptionInfo,
  SubscriptionProgressEvent,
} from '../../../shared/types/subscriptions';
import type { SubscriptionListMetrics } from '../../../shared/types/subscriptionsWorkspace';
import type { SubscriptionsSelection } from '../../../state/subscriptionsWorkspace';
import { SubscriptionCoverImage } from './SubscriptionCoverImage';
import { isSubscriptionCompleted, isSubscriptionUpToDate } from '../subscriptionUtils';
import { ActionButton } from './ActionButton';
import { useShortcutScope } from '../../../shared/hooks/useShortcutScope';
import styles from '../SubscriptionsScreen.module.css';

interface CardModel {
  selection: SubscriptionsSelection;
  key: string;
  name: string;
  files: number;
  sources: number;
  cover: SubscriptionCover | null;
  running: boolean;
  paused: boolean;
  attention: boolean;
  completed: boolean;
  upToDate: boolean;
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
  const dotClass = card.running
    ? styles.qDotRunning
    : card.attention
      ? styles.qDotAttention
      : card.paused
        ? styles.qDotPaused
        : card.completed
          ? styles.qDotSuccess
        : styles.qDotIdle;

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
          <SubscriptionCoverImage
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
            {card.name.slice(0, 1).toUpperCase()}
          </span>
        )}
      </span>
      <span className={styles.followName}>
        <span className={`${styles.qDot} ${dotClass}`.trim()} />
        <span className={styles.followNameText}>{card.name}</span>
        {card.completed && <span className={styles.upToDateChip}>{card.upToDate ? 'Up to date' : 'Completed'}</span>}
      </span>
      <span className={styles.followMeta}>
        {card.files.toLocaleString()} files
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
  listMetrics,
  covers,
  progressBySubscriptionId,
  runningSubscriptionIds,
  onSelect,
  onAdd,
  onOpenAccounts,
  onSubscriptionMenu,
  onMultiMenu,
}: {
  subscriptions: SubscriptionInfo[];
  listMetrics: Record<string, SubscriptionListMetrics>;
  covers: Map<string, SubscriptionCover>;
  progressBySubscriptionId: Map<string, SubscriptionProgressEvent>;
  runningSubscriptionIds: string[];
  onSelect: (selection: SubscriptionsSelection) => void;
  onAdd: () => void;
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
        files: sub.total_files,
        sources: sub.queries.length,
        cover: covers.get(sub.id) ?? null,
        running: running.has(sub.id) || progressBySubscriptionId.has(sub.id),
        paused: sub.paused,
        attention: hasAttention(sub.id),
        completed: !running.has(sub.id)
          && !progressBySubscriptionId.has(sub.id)
          && isSubscriptionCompleted(
            sub,
            listMetrics[sub.id]?.failedPostCount ?? 0,
            listMetrics[sub.id]?.openIssueCount ?? 0,
          ),
        upToDate: !running.has(sub.id)
          && !progressBySubscriptionId.has(sub.id)
          && isSubscriptionUpToDate(
            sub,
            listMetrics[sub.id]?.failedPostCount ?? 0,
            listMetrics[sub.id]?.openIssueCount ?? 0,
          ),
      })),
  ].sort((a, b) => b.files - a.files || a.name.localeCompare(b.name));

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
        <div className={styles.heroActions}>
          <ActionButton variant="primary" onClick={onAdd}>
            <IconPlus size={14} /> Add
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

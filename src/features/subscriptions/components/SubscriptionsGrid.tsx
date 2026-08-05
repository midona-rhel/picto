import { useEffect, useRef, useState } from 'react';
import { IconPlus, IconShieldLock } from '@tabler/icons-react';
import type {
  SubscriptionGroupInfo,
  SubscriptionInfo,
  SubscriptionProgressEvent,
} from '../../../shared/types/subscriptions';
import type { SubscriptionListMetrics } from '../../../shared/types/subscriptionsWorkspace';
import type { SubscriptionsSelection } from '../../../state/subscriptionsWorkspace';
import { mediaThumbnailUrl } from '../../../shared/lib/mediaUrl';
import { ActionButton } from './ActionButton';
import styles from '../SubscriptionsScreen.module.css';

interface CardModel {
  selection: SubscriptionsSelection;
  key: string;
  name: string;
  files: number;
  sources: number;
  coverHash: string | null;
  running: boolean;
  paused: boolean;
  attention: boolean;
}

/** Pick the newest member's cover as the group cover. */
function groupCover(group: SubscriptionGroupInfo, covers: Map<string, string>): string | null {
  const withCover = group.subscriptions
    .filter((sub) => covers.has(sub.id))
    .sort((a, b) => Number(b.id) - Number(a.id));
  return withCover.length > 0 ? covers.get(withCover[0].id) ?? null : null;
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
  const showCover = card.coverHash != null && !coverFailed;
  const dotClass = card.running
    ? styles.qDotRunning
    : card.attention
      ? styles.qDotAttention
      : card.paused
        ? styles.qDotPaused
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
          <img
            src={mediaThumbnailUrl(card.coverHash as string)}
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
      </span>
      <span className={styles.followMeta}>
        {card.files.toLocaleString()} files
        {card.sources > 0 && ` · ${card.sources} source${card.sources === 1 ? '' : 's'}`}
      </span>
    </button>
  );
}

/**
 * Subscription home — a card grid of subscriptions and groups (groups as
 * subjects, plus ungrouped subscriptions). Covers come from the newest
 * downloaded file.
 *
 * Interaction matches the main grid: click selects, double-click (or Enter)
 * opens, Cmd/Ctrl toggles, Shift range-selects, right-click opens the context
 * menu for the clicked card (or the whole selection when it's part of one).
 */
export function SubscriptionsGrid({
  groups,
  subscriptions,
  listMetrics,
  covers,
  progressBySubscriptionId,
  runningSubscriptionIds,
  onSelect,
  onAdd,
  onOpenAccounts,
  onSubscriptionMenu,
  onGroupMenu,
  onMultiMenu,
}: {
  groups: SubscriptionGroupInfo[];
  subscriptions: SubscriptionInfo[];
  listMetrics: Record<string, SubscriptionListMetrics>;
  covers: Map<string, string>;
  progressBySubscriptionId: Map<string, SubscriptionProgressEvent>;
  runningSubscriptionIds: string[];
  onSelect: (selection: SubscriptionsSelection) => void;
  onAdd: () => void;
  onOpenAccounts: () => void;
  onSubscriptionMenu: (position: { x: number; y: number }, id: string) => void;
  onGroupMenu: (position: { x: number; y: number }, id: string) => void;
  onMultiMenu: (
    position: { x: number; y: number },
    subscriptionIds: string[],
    groupIds: string[],
  ) => void;
}) {
  const grouped = new Set(groups.flatMap((group) => group.subscriptions.map((sub) => sub.id)));
  const running = new Set(runningSubscriptionIds);
  const hasAttention = (id: string) =>
    ((listMetrics[id]?.failedPostCount ?? 0) + (listMetrics[id]?.openIssueCount ?? 0)) > 0;

  const cards: CardModel[] = [
    ...groups.map((group): CardModel => ({
      selection: { kind: 'group', id: group.id },
      key: `group:${group.id}`,
      name: group.name,
      files: group.total_files,
      sources: group.subscriptions.reduce((sum, sub) => sum + Math.max(sub.queries.length, 1), 0),
      coverHash: groupCover(group, covers),
      running: group.subscriptions.some((sub) => running.has(sub.id) || progressBySubscriptionId.has(sub.id)),
      paused: group.subscriptions.length > 0 && group.subscriptions.every((sub) => sub.paused),
      attention: group.subscriptions.some((sub) => hasAttention(sub.id)),
    })),
    ...subscriptions
      .filter((sub) => !grouped.has(sub.id))
      .map((sub): CardModel => ({
        selection: { kind: 'subscription', id: sub.id },
        key: `sub:${sub.id}`,
        name: sub.name,
        files: sub.total_files,
        sources: sub.queries.length,
        coverHash: covers.get(sub.id) ?? null,
        running: running.has(sub.id) || progressBySubscriptionId.has(sub.id),
        paused: sub.paused,
        attention: hasAttention(sub.id),
      })),
  ].sort((a, b) => b.files - a.files || a.name.localeCompare(b.name));

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const anchorIndexRef = useRef<number | null>(null);

  // Drop selections for cards that no longer exist.
  const validKeys = new Set(cards.map((card) => card.key));
  const liveSelected = new Set([...selected].filter((key) => validKeys.has(key)));

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSelected(new Set());
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

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
      const subscriptionIds: string[] = [];
      const groupIds: string[] = [];
      for (const key of menuSelection) {
        const [kind, id] = key.split(':');
        if (kind === 'group') groupIds.push(id);
        else subscriptionIds.push(id);
      }
      onMultiMenu(position, subscriptionIds, groupIds);
      return;
    }
    if (card.selection?.kind === 'group') onGroupMenu(position, card.selection.id);
    else if (card.selection?.kind === 'subscription') onSubscriptionMenu(position, card.selection.id);
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

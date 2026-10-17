import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { IconCheck, IconX } from '@tabler/icons-react';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import styles from './InboxReviewControls.module.css';
import { t } from '../../i18n';

export type InboxReviewDecision = 'accept' | 'reject';

type ReviewableItem = { root_id: number; lifecycle: string } | null;

/** The viewed entity owns review eligibility, including inbox collections. */
export function resolveInboxReviewItemId(
  viewerItem: ReviewableItem,
  quickLookItem: ReviewableItem,
): number | null {
  if (quickLookItem?.lifecycle === 'inbox') return quickLookItem.root_id;
  if (viewerItem?.lifecycle === 'inbox') return viewerItem.root_id;
  return null;
}

interface InboxReviewControlsProps {
  itemId: number;
  /**
   * Review the item. The owner moves on to the next item at once and saves
   * in the background, coming back to this item if the save does not happen.
   * Returns false when it did not take the review (the item is already
   * being saved, or a question is open).
   */
  onReview: (itemId: number, decision: InboxReviewDecision) => boolean;
  /**
   * While a review waits on a question (a reject that deletes asks first),
   * the controls take no input, and their shortcuts reach nothing else.
   */
  paused?: boolean;
}

// Matches reviewFlash in the stylesheet.
const FLASH_DURATION_MS = 360;

/**
 * Accept and Reject for the item being viewed. The controls stay put from
 * one item to the next; a press only flashes the button it hit.
 */
export function InboxReviewControls({ itemId, onReview, paused = false }: InboxReviewControlsProps) {
  const [flash, setFlash] = useState<{ decision: InboxReviewDecision; key: number } | null>(null);
  const flashTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const acceptShortcut = getShortcut('inbox.accept')!;
  const rejectShortcut = getShortcut('inbox.reject')!;

  useEffect(() => () => clearTimeout(flashTimer.current), []);

  const review = useCallback((decision: InboxReviewDecision) => {
    // The owner refuses a second review of an item it is still saving.
    if (paused || !onReview(itemId, decision)) return;
    setFlash((current) => ({ decision, key: (current?.key ?? 0) + 1 }));
    clearTimeout(flashTimer.current);
    flashTimer.current = setTimeout(() => setFlash(null), FLASH_DURATION_MS);
  }, [itemId, onReview, paused]);

  useShortcutScope((event) => {
    if (event.repeat || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return;
    if (matchesShortcutDef(event, acceptShortcut)) {
      event.preventDefault();
      review('accept');
      return true;
    }
    if (matchesShortcutDef(event, rejectShortcut)) {
      event.preventDefault();
      review('reject');
      return true;
    }
  }, { priority: 90 });

  return createPortal(
    <div
      className={`${styles.pill} floatingGlassSurface`}
      data-inbox-review-controls
      data-review-decision={flash?.decision ?? 'idle'}
    >
      <KbdTooltip label={t("Accept")} shortcutId="inbox.accept" position="top">
        <button
          type="button"
          className={`${styles.action} ${styles.accept} ${flash?.decision === 'accept' ? styles.chosen : ''}`}
          aria-label={t("Accept item")}
          disabled={paused}
          onClick={() => review('accept')}
        >
          <span key={flash?.decision === 'accept' ? flash.key : 0} className={styles.glow} aria-hidden="true" />
          <IconCheck size={28} stroke={2} aria-hidden="true" />
        </button>
      </KbdTooltip>
      <span className={styles.divider} aria-hidden="true" />
      <KbdTooltip label={t("Reject")} shortcutId="inbox.reject" position="top">
        <button
          type="button"
          className={`${styles.action} ${styles.reject} ${flash?.decision === 'reject' ? styles.chosen : ''}`}
          aria-label={t("Reject item")}
          disabled={paused}
          onClick={() => review('reject')}
        >
          <span key={flash?.decision === 'reject' ? flash.key : 0} className={styles.glow} aria-hidden="true" />
          <IconX size={28} stroke={2} aria-hidden="true" />
        </button>
      </KbdTooltip>
    </div>,
    document.body,
  );
}

import { useCallback, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { IconCheck, IconX } from '@tabler/icons-react';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { showErrorNotification } from '../../shared/lib/notifications';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import styles from './InboxReviewControls.module.css';

export type InboxReviewDecision = 'accept' | 'reject';

type ReviewableItem = { item_id: number; lifecycle: string } | null;

/** The viewed entity owns review eligibility, including inbox collections. */
export function resolveInboxReviewItemId(
  viewerItem: ReviewableItem,
  quickLookItem: ReviewableItem,
): number | null {
  if (quickLookItem?.lifecycle === 'inbox') return quickLookItem.item_id;
  if (viewerItem?.lifecycle === 'inbox') return viewerItem.item_id;
  return null;
}

interface InboxReviewControlsProps {
  itemId: number;
  onCommit: (itemId: number, decision: InboxReviewDecision) => Promise<void>;
  onAdvance: () => void;
}

const EXIT_DURATION_MS = 120;

export function InboxReviewControls({ itemId, onCommit, onAdvance }: InboxReviewControlsProps) {
  const [decision, setDecision] = useState<InboxReviewDecision | null>(null);
  const [leaving, setLeaving] = useState(false);
  const acceptShortcut = getShortcut('inbox.accept')!;
  const rejectShortcut = getShortcut('inbox.reject')!;

  useEffect(() => {
    setDecision(null);
    setLeaving(false);
  }, [itemId]);

  const review = useCallback(async (nextDecision: InboxReviewDecision) => {
    if (decision) return;
    setDecision(nextDecision);
    try {
      await onCommit(itemId, nextDecision);
      setLeaving(true);
      await new Promise<void>((resolve) => setTimeout(resolve, EXIT_DURATION_MS));
      onAdvance();
    } catch (reason) {
      setDecision(null);
      setLeaving(false);
      showErrorNotification({
        title: nextDecision === 'accept' ? 'Unable to accept item' : 'Unable to reject item',
        message: reason instanceof Error ? reason.message : String(reason),
      });
    }
  }, [decision, itemId, onAdvance, onCommit]);

  useShortcutScope((event) => {
    if (event.repeat || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return;
    if (matchesShortcutDef(event, acceptShortcut)) {
      event.preventDefault();
      void review('accept');
      return true;
    }
    if (matchesShortcutDef(event, rejectShortcut)) {
      event.preventDefault();
      void review('reject');
      return true;
    }
  }, { priority: 90 });

  return createPortal(
    <div
      className={`${styles.pill} floatingGlassSurface ${leaving ? styles.leaving : ''}`}
      data-inbox-review-controls
      data-review-decision={decision ?? 'idle'}
    >
      <KbdTooltip label="Accept" shortcutId="inbox.accept" position="top">
        <button
          type="button"
          className={`${styles.action} ${styles.accept} ${decision === 'accept' ? styles.chosen : ''}`}
          aria-label="Accept item"
          disabled={decision != null}
          onClick={() => { void review('accept'); }}
        >
          <IconCheck size={28} stroke={2} aria-hidden="true" />
        </button>
      </KbdTooltip>
      <span className={styles.divider} aria-hidden="true" />
      <KbdTooltip label="Reject" shortcutId="inbox.reject" position="top">
        <button
          type="button"
          className={`${styles.action} ${styles.reject} ${decision === 'reject' ? styles.chosen : ''}`}
          aria-label="Reject item"
          disabled={decision != null}
          onClick={() => { void review('reject'); }}
        >
          <IconX size={28} stroke={2} aria-hidden="true" />
        </button>
      </KbdTooltip>
    </div>,
    document.body,
  );
}

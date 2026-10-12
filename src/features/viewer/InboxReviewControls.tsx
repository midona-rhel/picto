import { useCallback, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { IconCheck, IconX } from '@tabler/icons-react';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { showErrorNotification } from '../../shared/lib/notifications';
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
  onCommit: (itemId: number, decision: InboxReviewDecision) => Promise<void>;
  onAdvance: () => void;
}

// Long enough for the press pulse to read before the pill leaves.
const EXIT_DURATION_MS = 220;

export function InboxReviewControls({ itemId, onCommit, onAdvance }: InboxReviewControlsProps) {
  const [decision, setDecision] = useState<InboxReviewDecision | null>(null);
  const [leaving, setLeaving] = useState(false);
  // The press light outlives the item: it keeps running on the pill while
  // the next item comes in, so reviewing never waits for it.
  const [burst, setBurst] = useState<{ decision: InboxReviewDecision; id: number } | null>(null);
  const acceptShortcut = getShortcut('inbox.accept')!;
  const rejectShortcut = getShortcut('inbox.reject')!;

  useEffect(() => {
    setDecision(null);
    setLeaving(false);
  }, [itemId]);

  const review = useCallback(async (nextDecision: InboxReviewDecision) => {
    if (decision) return;
    setDecision(nextDecision);
    setBurst((current) => ({ decision: nextDecision, id: (current?.id ?? 0) + 1 }));
    try {
      await onCommit(itemId, nextDecision);
      setLeaving(true);
      await new Promise<void>((resolve) => setTimeout(resolve, EXIT_DURATION_MS));
      onAdvance();
    } catch (reason) {
      setDecision(null);
      setLeaving(false);
      showErrorNotification({
        title: nextDecision === 'accept' ? t('Unable to accept item') : t('Unable to reject item'),
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
      <span className={styles.burstClip} aria-hidden="true">
        {burst ? (
          <span
            key={burst.id}
            className={`${styles.burst} ${burst.decision === 'accept' ? styles.burstAccept : styles.burstReject}`}
            data-review-burst={burst.decision}
          >
            <span
              className={styles.burstCloud}
              onAnimationEnd={() => setBurst((current) => (current?.id === burst.id ? null : current))}
            />
            <span className={styles.burstRim}>
              <span className={styles.burstRimNear} />
              <span className={styles.burstRimFar} />
            </span>
          </span>
        ) : null}
      </span>
      <KbdTooltip label={t("Accept")} shortcutId="inbox.accept" position="top">
        <button
          type="button"
          className={`${styles.action} ${styles.accept} ${decision === 'accept' ? styles.chosen : ''}`}
          aria-label={t("Accept item")}
          disabled={decision != null}
          onClick={() => { void review('accept'); }}
        >
          <span className={styles.glow} aria-hidden="true" />
          <IconCheck size={28} stroke={2} aria-hidden="true" />
        </button>
      </KbdTooltip>
      <span className={styles.divider} aria-hidden="true" />
      <KbdTooltip label={t("Reject")} shortcutId="inbox.reject" position="top">
        <button
          type="button"
          className={`${styles.action} ${styles.reject} ${decision === 'reject' ? styles.chosen : ''}`}
          aria-label={t("Reject item")}
          disabled={decision != null}
          onClick={() => { void review('reject'); }}
        >
          <span className={styles.glow} aria-hidden="true" />
          <IconX size={28} stroke={2} aria-hidden="true" />
        </button>
      </KbdTooltip>
    </div>,
    document.body,
  );
}

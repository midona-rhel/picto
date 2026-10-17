/**
 * Accepting or rejecting the Inbox item under review.
 *
 * The view moves to the next item at once, chosen by id, and the change is
 * saved in the background: waiting for the save made every review take most
 * of a second, and finding the place by index after the item had left the
 * list could briefly show the wrong item (one an undo had just put back).
 * If the save fails or is cancelled, the view goes back to the item, but only
 * while it is still where this review left it.
 */
import type { InboxReviewDecision } from '../viewer/InboxReviewControls';

export type ReviewView = 'quicklook' | 'detail';

export interface InboxReviewHost {
  /** The items in view order. */
  items(): readonly { root_id: number }[];
  /** The view the review happens in. */
  view(): ReviewView;
  /** The item `view` shows, or null when it is closed. */
  shownItemId(view: ReviewView): number | null;
  /** Show and select `itemId` in `view`, or close `view` for null. */
  show(view: ReviewView, itemId: number | null): void;
  clearSelection(): void;
  /** A question (a reject that deletes asks first) is waiting for an answer. */
  isAsking(): boolean;
  /** Stop a scroll that is still coasting from carrying into the next item. */
  suppressScrollMomentum(): void;
  /** Save the review; false when it was cancelled. */
  save(itemId: number, decision: InboxReviewDecision): Promise<boolean>;
  reportFailure(decision: InboxReviewDecision, reason: unknown): void;
}

export interface InboxReviewer {
  /** False when the review was not taken (a question is open, or it is already saving). */
  review(itemId: number, decision: InboxReviewDecision): boolean;
  /** Call when the list changes, so saved items stop being skipped once gone. */
  itemsChanged(): void;
}

export function createInboxReviewer(host: InboxReviewHost): InboxReviewer {
  // Items whose review is still being saved, and saved items the list has
  // not dropped yet: the review must not land on them again.
  const inFlight = new Set<number>();
  const leaving = new Set<number>();
  let latest: { view: ReviewView; itemId: number | null } | null = null;

  const isListed = (itemId: number) => host.items().some((item) => item.root_id === itemId);

  function review(itemId: number, decision: InboxReviewDecision): boolean {
    if (host.isAsking() || inFlight.has(itemId)) return false;
    const items = host.items();
    const index = items.findIndex((item) => item.root_id === itemId);
    const open = (item: { root_id: number }) =>
      item.root_id !== itemId && !inFlight.has(item.root_id) && !leaving.has(item.root_id);
    const nextItemId = index < 0
      ? null
      : items.slice(index + 1).find(open)?.root_id
        ?? items.slice(0, index).reverse().find(open)?.root_id
        ?? null;
    inFlight.add(itemId);

    const view = host.view();
    host.suppressScrollMomentum();
    host.show(view, nextItemId);
    // With nothing left to show, the departing item must not stay selected
    // while it saves; a failed save reselects it when it comes back.
    if (nextItemId == null) host.clearSelection();
    const destination = { view, itemId: nextItemId };
    latest = destination;

    const returnToItem = () => {
      if (latest !== destination) return;
      latest = null;
      // The last item closes the view, so "still there" is "still closed".
      if (host.shownItemId(view) === destination.itemId && isListed(itemId)) host.show(view, itemId);
    };
    host.save(itemId, decision)
      .then((committed) => {
        inFlight.delete(itemId);
        if (!committed) returnToItem();
        else if (isListed(itemId)) leaving.add(itemId);
      })
      .catch((reason: unknown) => {
        inFlight.delete(itemId);
        returnToItem();
        host.reportFailure(decision, reason);
      });
    return true;
  }

  function itemsChanged(): void {
    for (const itemId of leaving) {
      if (!isListed(itemId)) leaving.delete(itemId);
    }
  }

  return { review, itemsChanged };
}

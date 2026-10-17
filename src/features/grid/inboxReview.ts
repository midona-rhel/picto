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
  /** The loaded items in view order (a window of the whole list). */
  items(): readonly { root_id: number }[];
  /** Where the loaded window starts in the whole list. */
  windowStart(): number;
  /** More items follow the loaded window. */
  hasFollowing(): boolean;
  /** Load the window that follows the loaded one. */
  loadFollowing(): Promise<void>;
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
  let latest: { itemId: number | null; moved: boolean } | null = null;

  const isListed = (itemId: number) => host.items().some((item) => item.root_id === itemId);

  function review(itemId: number, decision: InboxReviewDecision): boolean {
    if (host.isAsking() || inFlight.has(itemId)) return false;
    const open = (item: { root_id: number }) =>
      item.root_id !== itemId && !inFlight.has(item.root_id) && !leaving.has(item.root_id);
    // The reviewed item's place in the whole list. A window loaded after its
    // save landed no longer holds it, but its successor then takes its place.
    const loadedIndex = host.items().findIndex((item) => item.root_id === itemId);
    const position = loadedIndex < 0 ? null : host.windowStart() + loadedIndex;
    /** Where the item is, or would be, in the loaded window, and whether it is there. */
    const place = () => {
      const items = host.items();
      const index = items.findIndex((item) => item.root_id === itemId);
      if (index >= 0) return { items, index, present: true };
      if (position == null) return null;
      return { items, index: Math.max(0, position - host.windowStart()), present: false };
    };
    const following = () => {
      const at = place();
      return at?.items.slice(at.present ? at.index + 1 : at.index).find(open)?.root_id;
    };
    const preceding = () => {
      const at = place();
      return at?.items.slice(0, at.index).reverse().find(open)?.root_id;
    };
    inFlight.add(itemId);

    const view = host.view();
    // Where this review left the view; null until it has moved on.
    const destination: { itemId: number | null; moved: boolean } = { itemId: null, moved: false };
    latest = destination;
    const moveOn = (nextItemId: number | null) => {
      if (latest !== destination || destination.moved) return;
      destination.itemId = nextItemId;
      destination.moved = true;
      host.suppressScrollMomentum();
      host.show(view, nextItemId);
      // With nothing left to show, the departing item must not stay selected
      // while it saves; a failed save reselects it when it comes back.
      if (nextItemId == null) host.clearSelection();
    };

    const next = following();
    if (next != null || !host.hasFollowing()) moveOn(next ?? preceding() ?? null);
    else {
      // The item is the last one loaded but not the last in the list: the
      // next one is in the following window, not behind.
      host.loadFollowing()
        .catch(() => {})
        .then(() => {
          // Only while the view is still on the reviewed item.
          if (host.shownItemId(view) === itemId) moveOn(following() ?? preceding() ?? null);
        });
    }

    const returnToItem = () => {
      if (latest !== destination) return;
      latest = null;
      if (!destination.moved) return;
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

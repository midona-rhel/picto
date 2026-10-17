import { describe, expect, it, vi } from 'vitest';
import { createInboxReviewer, type InboxReviewHost, type ReviewView } from './inboxReview';

/** A host with a real list and view, and saves the test settles by hand. */
function setup(ids: number[], view: ReviewView = 'quicklook', following: number[] = []) {
  let items = ids.map((root_id) => ({ root_id }));
  let pendingWindow = following.map((root_id) => ({ root_id }));
  let finishLoad: (() => void) | null = null;

  let shown: number | null = ids[0] ?? null;
  let selected: number | null = shown;
  let asking = false;
  const saves = new Map<number, { resolve: (committed: boolean) => void; reject: (reason: unknown) => void }>();
  const host: InboxReviewHost = {
    items: () => items,
    hasFollowing: () => pendingWindow.length > 0,
    loadFollowing: vi.fn(() => new Promise<void>((resolve) => {
      finishLoad = () => {
        items = [...items, ...pendingWindow];
        pendingWindow = [];
        resolve();
      };
    })),
    view: () => view,
    shownItemId: () => shown,
    show: vi.fn((_view, itemId) => { shown = itemId; selected = itemId; }),
    clearSelection: vi.fn(() => { selected = null; }),
    isAsking: () => asking,
    suppressScrollMomentum: vi.fn(),
    save: vi.fn((itemId) => new Promise<boolean>((resolve, reject) => { saves.set(itemId, { resolve, reject }); })),
    reportFailure: vi.fn(),
  };
  const reviewer = createInboxReviewer(host);
  const settle = async () => { await new Promise((resolve) => setTimeout(resolve, 0)); };
  return {
    host,
    reviewer,
    saves,
    settle,
    get shown() { return shown; },
    get selected() { return selected; },
    navigate(itemId: number | null) { shown = itemId; selected = itemId; },
    ask(value: boolean) { asking = value; },
    async loadWindow() { finishLoad?.(); await settle(); },
    /** The list drops saved items, as a refresh after the save does. */
    drop(...gone: number[]) {
      items = items.filter((item) => !gone.includes(item.root_id));
      reviewer.itemsChanged();
    },
  };
}

describe('createInboxReviewer', () => {
  it('moves to the next item at once and saves in the background', () => {
    const review = setup([1, 2, 3]);
    review.reviewer.review(1, 'accept');
    expect(review.shown).toBe(2);
    expect(review.host.save).toHaveBeenCalledWith(1, 'accept');
    expect(review.host.suppressScrollMomentum).toHaveBeenCalled();
  });

  it('comes back to the item when its save is cancelled or fails', async () => {
    const review = setup([1, 2, 3]);
    review.reviewer.review(1, 'reject');
    review.saves.get(1)!.resolve(false);
    await review.settle();
    expect(review.shown).toBe(1);

    review.reviewer.review(1, 'accept');
    expect(review.shown).toBe(2);
    review.saves.get(1)!.reject(new Error('disk full'));
    await review.settle();
    expect(review.shown).toBe(1);
    expect(review.host.reportFailure).toHaveBeenCalledWith('accept', expect.any(Error));
  });

  it('reopens the last item after its failed save closed the view', async () => {
    const review = setup([1]);
    review.reviewer.review(1, 'accept');
    expect(review.shown).toBeNull();
    expect(review.selected).toBeNull();
    review.saves.get(1)!.reject(new Error('locked'));
    await review.settle();
    expect(review.shown).toBe(1);
    expect(review.selected).toBe(1);
  });

  it('stays put when the view has moved on before the save fails', async () => {
    const review = setup([1, 2, 3]);
    review.reviewer.review(1, 'accept');
    review.navigate(3);
    review.saves.get(1)!.resolve(false);
    await review.settle();
    expect(review.shown).toBe(3);
  });

  it('only lets the latest review bring its item back', async () => {
    const review = setup([1, 2, 3]);
    review.reviewer.review(1, 'accept');
    review.reviewer.review(2, 'accept');
    review.saves.get(1)!.resolve(false);
    await review.settle();
    expect(review.shown).toBe(3);
  });

  it('never steps back onto an item that is still saving', async () => {
    const review = setup([1, 2]);
    review.reviewer.review(1, 'accept');
    review.reviewer.review(2, 'accept');
    // Both are saving: nothing is left to show.
    expect(review.shown).toBeNull();
    expect(review.reviewer.review(1, 'accept')).toBe(false);
    expect(review.host.save).toHaveBeenCalledTimes(2);
  });

  it('skips a saved item until the list drops it, then forgets it', async () => {
    const review = setup([1, 2, 3]);
    review.reviewer.review(2, 'accept');
    review.saves.get(2)!.resolve(true);
    await review.settle();
    review.navigate(3);
    review.reviewer.review(3, 'accept');
    // 2 is saved but still listed, so the review goes to 1.
    expect(review.shown).toBe(1);
    review.drop(2, 3);
  });

  it('takes the item again once its save fails, even if it is already back in view', async () => {
    const review = setup([1, 2, 3]);
    review.reviewer.review(1, 'accept');
    review.navigate(1);
    expect(review.reviewer.review(1, 'accept')).toBe(false);
    review.saves.get(1)!.reject(new Error('locked'));
    await review.settle();
    expect(review.shown).toBe(1);
    expect(review.reviewer.review(1, 'reject')).toBe(true);
  });

  it('loads the following window instead of stepping back at its edge', async () => {
    const review = setup([1, 2], 'quicklook', [3, 4]);
    review.navigate(2);
    review.reviewer.review(2, 'accept');
    // Still on 2 while the next window loads, never back to 1.
    expect(review.shown).toBe(2);
    expect(review.host.loadFollowing).toHaveBeenCalled();
    await review.loadWindow();
    expect(review.shown).toBe(3);
  });

  it('saves an edge item only after the following window is loaded', async () => {
    const review = setup([1, 2], 'quicklook', [3, 4]);
    review.navigate(2);
    review.reviewer.review(2, 'reject');
    // Saving first could drop the item from the new window, or close the
    // view on a delete, before the review has found where to go.
    expect(review.host.save).not.toHaveBeenCalled();
    await review.loadWindow();
    expect(review.shown).toBe(3);
    expect(review.host.save).toHaveBeenCalledWith(2, 'reject');
  });

  it('stays where the user went while the following window loads', async () => {
    const review = setup([1, 2], 'quicklook', [3]);
    review.navigate(2);
    review.reviewer.review(2, 'accept');
    review.navigate(1);
    await review.loadWindow();
    expect(review.shown).toBe(1);
  });

  it('takes no review while a question is waiting', () => {
    const review = setup([1, 2]);
    review.ask(true);
    expect(review.reviewer.review(1, 'reject')).toBe(false);
    expect(review.shown).toBe(1);
    expect(review.host.save).not.toHaveBeenCalled();
    review.ask(false);
    review.reviewer.review(1, 'reject');
    expect(review.shown).toBe(2);
  });
});

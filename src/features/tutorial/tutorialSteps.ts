export type TutorialPlacement = 'top' | 'right' | 'bottom' | 'left';

export type TutorialChapter =
  | 'sidebar' | 'all-media' | 'inspector' | 'inbox' | 'folders'
  | 'tags' | 'collections' | 'subscriptions' | 'duplicates' | 'trash';

export type TutorialAction =
  | { type: 'navigate'; nodeId: string }
  | { type: 'select_first' }
  | { type: 'select_first_kind'; kind: 'media' | 'collection' }
  | { type: 'viewer'; mode: 'detail' | 'quick-look' | 'close' }
  | { type: 'set_first_lifecycle'; lifecycle: 'active' | 'inbox' | 'trash' }
  | { type: 'restore_last_lifecycle'; lifecycle: 'active' | 'inbox' | 'trash' }
  | { type: 'set_tutorial_subscription_runs'; count: 0 | 1 | 2 }
  | { type: 'restore_rejected_item' }
  | { type: 'set_tutorial_folder_membership'; present: boolean }
  | { type: 'set_tutorial_tag'; present: boolean }
  | { type: 'set_tutorial_collection_order'; reversed: boolean };

export type TutorialCondition =
  | { type: 'grid_items'; minimum: number }
  | { type: 'subscription_idle' }
  | { type: 'duplicates_ready' };

export interface TutorialStep {
  id: string;
  chapter: TutorialChapter;
  target: string;
  placement: TutorialPlacement;
  title: string;
  description: string;
  enter?: TutorialAction[];
  leaveBackward?: TutorialAction[];
  waitFor?: TutorialCondition;
}

const step = (
  id: string,
  chapter: TutorialChapter,
  target: string,
  title: string,
  description: string,
  enter?: TutorialAction[],
  waitFor?: TutorialCondition,
  placement: TutorialPlacement = 'right',
  leaveBackward?: TutorialAction[],
): TutorialStep => ({ id, chapter, target, title, description, enter, waitFor, placement, leaveBackward });

/** The sequence controls real Picto surfaces; it never renders tutorial copies. */
export const GUIDED_TOUR_STEPS: readonly TutorialStep[] = [
  step('sidebar', 'sidebar', 'sidebar', 'Browse your library', 'The sidebar keeps every destination close: your whole library, new arrivals, folders, saved searches, subscriptions, duplicates, and trash.'),
  step('library', 'sidebar', 'sidebar-library-switcher', 'Your tutorial library', 'The tour is running in a temporary library. Your real library is safe and will return exactly as you left it when you exit.'),
  step('all-destination', 'sidebar', 'sidebar-all', 'All media', 'All media contains everything you have kept. Items still waiting in Inbox and items in trash stay out of this view.', [{ type: 'navigate', nodeId: 'system:active' }]),
  step('inbox-destination', 'sidebar', 'sidebar-inbox', 'Inbox', 'New imports and subscription downloads arrive here so you can decide what to keep.', [{ type: 'navigate', nodeId: 'system:inbox' }]),
  step('recent-destination', 'sidebar', 'sidebar-recently-viewed', 'Recently viewed', 'Return to media you opened recently. You can clear this history from its context menu.', [{ type: 'navigate', nodeId: 'system:recent_viewed' }]),
  step('uncategorized-destination', 'sidebar', 'sidebar-uncategorized', 'Uncategorized', 'Find media that has not been put in a folder yet. Adding it to any folder removes it from this list.', [{ type: 'navigate', nodeId: 'system:uncategorized' }]),
  step('untagged-destination', 'sidebar', 'sidebar-untagged', 'Untagged', 'Find media without tags. Add a tag yourself or review automatic suggestions.', [{ type: 'navigate', nodeId: 'system:untagged' }]),
  step('tags-destination', 'sidebar', 'sidebar-tags', 'Tags', 'Browse and maintain the words used to describe media across this library.', [{ type: 'navigate', nodeId: 'system:tag_manager' }]),
  step('random-destination', 'sidebar', 'sidebar-random', 'Random', 'Rediscover things you forgot you saved with a shuffled view of the library.', [{ type: 'navigate', nodeId: 'system:random' }]),
  step('subscriptions-destination', 'sidebar', 'sidebar-subscriptions', 'Subscriptions', 'Subscriptions check creators and feeds for new posts, then place the downloads in Inbox for review.', [{ type: 'navigate', nodeId: 'system:subscriptions' }]),
  step('duplicates-destination', 'sidebar', 'sidebar-duplicates', 'Duplicates', 'Review visually similar files and decide which copy should keep the combined metadata.', [{ type: 'navigate', nodeId: 'system:duplicates' }]),
  step('trash-destination', 'sidebar', 'sidebar-trash', 'Trash', 'Restore media you still want, or permanently delete it when you are certain it is no longer needed.', [{ type: 'navigate', nodeId: 'system:trash' }]),
  step('quick-access', 'sidebar', 'sidebar-quick-access', 'Quick access', 'Pin the folders and saved searches you use most so they remain one click away.'),
  step('folders-sidebar', 'sidebar', 'sidebar-folders', 'Folders', 'Folders organize media without making copies. One item can belong to several folders, and folders can be nested.'),
  step('smart-sidebar', 'sidebar', 'sidebar-smart-folders', 'Smart folders', 'Smart folders save rules instead of moving files. A child can only narrow the results supplied by its parent group.'),

  step('all-grid', 'all-media', 'workspace', 'A real media grid', 'These are real items in the temporary library, rendered by the same canvas and thumbnail pipeline as your own media.', [{ type: 'navigate', nodeId: 'system:active' }], { type: 'grid_items', minimum: 2 }, 'top'),
  step('select-media', 'all-media', 'inspector', 'Select an item', 'Selecting media shows its preview and organization controls in the inspector without leaving the grid.', [{ type: 'select_first' }], undefined, 'left'),
  step('detail-media', 'all-media', 'workspace', 'Detail view', 'Detail view gives one item the workspace while keeping the normal toolbar and inspector available.', [{ type: 'viewer', mode: 'detail' }], undefined, 'top'),
  step('quick-media', 'all-media', 'workspace', 'Quick Look', 'Quick Look gives the preview the whole available window, then returns to the same grid position.', [{ type: 'viewer', mode: 'quick-look' }], undefined, 'top'),
  step('return-media', 'all-media', 'workspace', 'Back where you started', 'Closing either viewer returns to the same item and scroll position.', [{ type: 'viewer', mode: 'close' }], undefined, 'top'),

  step('inspector-overview', 'inspector', 'inspector', 'Inspect and organize', 'Edit the name, notes, source, rating, tags, and folder membership here. Properties below describe the original file.', [{ type: 'select_first' }], undefined, 'left'),
  step('inspector-actions', 'inspector', 'inspector', 'Actions stay contextual', 'Available actions change with the selection while the inspector itself stays in place.'),

  step('inbox-open', 'inbox', 'workspace', 'Review new arrivals', 'Inbox is oldest first so review stays predictable. This fixture arrived through the real ingest queue.', [{ type: 'navigate', nodeId: 'system:inbox' }], { type: 'grid_items', minimum: 1 }, 'top'),
  step('inbox-accept', 'inbox', 'workspace', 'Keep an item', 'Accepting moves the selected item into your active library. The next step performs that real reversible action.', [{ type: 'select_first' }, { type: 'set_first_lifecycle', lifecycle: 'active' }], undefined, 'top', [{ type: 'restore_last_lifecycle', lifecycle: 'inbox' }]),
  step('inbox-reject', 'inbox', 'workspace', 'Reject an item', 'This item has moved to trash. It is no longer waiting for review, but it has not been permanently deleted.', [{ type: 'navigate', nodeId: 'system:inbox' }, { type: 'select_first' }, { type: 'set_first_lifecycle', lifecycle: 'trash' }], undefined, 'top', [{ type: 'restore_last_lifecycle', lifecycle: 'inbox' }]),

  step('folder-real', 'folders', 'sidebar-folders', 'A real folder tree', 'Renaissance reference contains Portrait studies. Dragging media into a folder adds membership; it does not duplicate the file.'),
  step('folder-multiple', 'folders', 'inspector', 'Use more than one folder', 'This item now belongs to both tutorial folders. Membership changes where it appears without copying the file.', [{ type: 'navigate', nodeId: 'system:active' }, { type: 'select_first_kind', kind: 'media' }, { type: 'set_tutorial_folder_membership', present: true }], undefined, 'left', [{ type: 'set_tutorial_folder_membership', present: false }]),
  step('smart-real', 'folders', 'sidebar-smart-folders', 'Saved rules', 'Smart folders update automatically as media changes. Groups let several saved searches share the same starting set.'),

  step('tags-add', 'tags', 'inspector', 'Add and remove tags', 'This real item has received a tutorial tag. The same tag controls filters, counts, and smart-folder membership everywhere.', [{ type: 'navigate', nodeId: 'system:active' }, { type: 'select_first_kind', kind: 'media' }, { type: 'set_tutorial_tag', present: true }], undefined, 'left', [{ type: 'set_tutorial_tag', present: false }]),
  step('tags-manager', 'tags', 'workspace', 'Your tag vocabulary', 'Tags are grouped by meaning, can be favorited, renamed or merged, and show how many visible media items use them.', [{ type: 'navigate', nodeId: 'system:tag_manager' }], undefined, 'top'),
  step('system-views', 'tags', 'sidebar', 'Views that organize themselves', 'Uncategorized, untagged, recently viewed, random, and tag filters are computed from your library—nothing is copied.'),

  step('collections', 'collections', 'workspace', 'Collections keep related media together', 'This real collection occupies one grid tile and opens into its ordered members.', [{ type: 'navigate', nodeId: 'system:active' }, { type: 'select_first_kind', kind: 'collection' }, { type: 'viewer', mode: 'detail' }], { type: 'grid_items', minimum: 2 }, 'top'),
  step('collections-reorder', 'collections', 'workspace', 'Keep a meaningful order', 'The two real members have swapped positions through the production collection command. Previous restores their original order.', [{ type: 'set_tutorial_collection_order', reversed: true }], undefined, 'top', [{ type: 'set_tutorial_collection_order', reversed: false }]),
  step('collections-return', 'collections', 'workspace', 'Return to the originating grid', 'Closing the collection returns to the same grid and selection that opened it.', [{ type: 'viewer', mode: 'close' }], undefined, 'top'),

  step('subscriptions-overview', 'subscriptions', 'workspace', 'A real offline subscription', 'Leonardo da Vinci Archive is stored like any other Twitter/X subscription, but this tutorial runner is deliberately offline.', [{ type: 'navigate', nodeId: 'system:subscriptions' }], undefined, 'top'),
  step('subscriptions-run', 'subscriptions', 'workspace', 'Downloads continue in the background', 'This run emits a bundled Mona Lisa post through the real progress, history, download, thumbnail, tagging, and Inbox pipeline.', [{ type: 'set_tutorial_subscription_runs', count: 1 }], { type: 'subscription_idle' }, 'top', [{ type: 'set_tutorial_subscription_runs', count: 0 }]),
  step('subscriptions-inbox', 'subscriptions', 'workspace', 'The new post has arrived', 'The subscription download is now a real Inbox item with its source, automatic tags, and destination folder.', [{ type: 'navigate', nodeId: 'system:inbox' }], { type: 'grid_items', minimum: 1 }, 'top'),
  step('subscriptions-group', 'subscriptions', 'workspace', 'Posts can become collections', 'A second run emits two images from one post. Group-post handling turns them into one real ordered collection.', [{ type: 'navigate', nodeId: 'system:subscriptions' }, { type: 'set_tutorial_subscription_runs', count: 2 }], { type: 'subscription_idle' }, 'top', [{ type: 'set_tutorial_subscription_runs', count: 1 }]),

  step('duplicates', 'duplicates', 'workspace', 'Compare likely duplicates', 'Picto loads the full-quality pair, shows their similarity and difference view, and previews what Smart Merge would keep before any deletion.', [{ type: 'navigate', nodeId: 'system:duplicates' }], { type: 'duplicates_ready' }, 'top'),

  step('trash', 'trash', 'workspace', 'Restore before deleting forever', 'The rejected Inbox item is here and remains recoverable. This tour never performs permanent deletion.', [{ type: 'navigate', nodeId: 'system:inbox' }, { type: 'select_first' }, { type: 'set_first_lifecycle', lifecycle: 'trash' }, { type: 'navigate', nodeId: 'system:trash' }], { type: 'grid_items', minimum: 1 }, 'top', [{ type: 'restore_last_lifecycle', lifecycle: 'inbox' }]),
  step('trash-restore', 'trash', 'workspace', 'Restore an item', 'Restoring returns the selected item to the active library. The tour performs that reversible action now.', [{ type: 'restore_rejected_item' }], undefined, 'top'),
  step('complete', 'trash', 'sidebar', 'Your library is ready', 'Exit returns to your original library, view, filters, selection, panels, viewer, and scroll position. Reopening this tour starts over.'),
] as const;

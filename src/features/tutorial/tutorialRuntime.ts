import { getDefaultStore } from 'jotai';
import { invoke } from '../../platform/ipc';
import {
  captureNavigationSession, navigateToNode, resetNavigationHistory, restoreNavigationSession,
  type NavigationSessionSnapshot,
} from '../../state/navigationHistory';
import { gridController } from '../../controllers/gridController';
import { nodeIdToGridScope } from '../../shared/lib/gridScope';
import { gridItemsAtom, gridSessionAtom, type QueryFilters } from '../../state/grid';
import { gridSelectionAtom, emptyGridSelection, type GridSelection } from '../../state/selection';
import {
  activeNodeIdAtom, displayedSurfaceNodeIdAtom, inspectorCollapsedAtom, sidebarCollapsedAtom,
} from '../../state/navigation';
import { quickLookSessionAtom, viewerSessionAtom, type ViewerSession } from '../../state/viewer';
import type { TutorialAction, TutorialCondition } from './tutorialSteps';

const store = getDefaultStore();

interface TutorialUiSnapshot {
  nodeId: string;
  searchText: string;
  filters: QueryFilters;
  selection: GridSelection;
  sidebarCollapsed: boolean;
  inspectorCollapsed: boolean;
  viewer: ViewerSession | null;
  quickLook: ViewerSession | null;
  scrollTop: number;
  navigation: NavigationSessionSnapshot;
}

let snapshot: TutorialUiSnapshot | null = null;
let lastLifecycleItemId: number | null = null;
const originalCollectionOrder = new Map<number, number[]>();

function cloneSelection(selection: TutorialUiSnapshot['selection']): TutorialUiSnapshot['selection'] {
  return {
    ...selection,
    itemIds: new Set(selection.itemIds),
    excludedItemIds: new Set(selection.excludedItemIds),
    folderNodeIds: new Set(selection.folderNodeIds),
  };
}

export async function startTutorialSession(): Promise<void> {
  const session = store.get(gridSessionAtom);
  snapshot = {
    nodeId: store.get(displayedSurfaceNodeIdAtom),
    searchText: session.searchText,
    filters: { ...session.filters, include_tags: [...session.filters.include_tags], exclude_tags: [...session.filters.exclude_tags] },
    selection: cloneSelection(store.get(gridSelectionAtom)),
    sidebarCollapsed: store.get(sidebarCollapsedAtom),
    inspectorCollapsed: store.get(inspectorCollapsedAtom),
    viewer: store.get(viewerSessionAtom),
    quickLook: store.get(quickLookSessionAtom),
    scrollTop: document.querySelector<HTMLElement>('[data-grid-scroll-container]')?.scrollTop ?? 0,
    navigation: captureNavigationSession(),
  };
  let opened = false;
  try {
    await (window as any).picto.tutorial.start();
    opened = true;
    store.set(viewerSessionAtom, null);
    store.set(quickLookSessionAtom, null);
    store.set(gridSelectionAtom, emptyGridSelection());
    store.set(sidebarCollapsedAtom, false);
    store.set(inspectorCollapsedAtom, false);
    resetNavigationHistory('system:active');
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    await waitUntil(() => document.querySelector('[data-help-id="sidebar"]') != null, 20_000);
  } catch (error) {
    if (opened) {
      await finishTutorialSession().catch(() => undefined);
    } else {
      snapshot = null;
    }
    throw error;
  }
}

export async function finishTutorialSession(): Promise<void> {
  const restore = snapshot;
  await (window as any).picto.tutorial.finish();
  snapshot = null;
  if (!restore) return;
  store.set(sidebarCollapsedAtom, restore.sidebarCollapsed);
  store.set(inspectorCollapsedAtom, restore.inspectorCollapsed);
  restoreNavigationSession(restore.navigation);
  await waitUntil(() => store.get(displayedSurfaceNodeIdAtom) === restore.nodeId, 5_000);
  gridController.setSearchText(restore.searchText);
  store.set(gridSelectionAtom, cloneSelection(restore.selection));
  store.set(viewerSessionAtom, restore.viewer);
  store.set(quickLookSessionAtom, restore.quickLook);
  requestAnimationFrame(() => {
    const container = document.querySelector<HTMLElement>('[data-grid-scroll-container]');
    if (container) container.scrollTop = restore.scrollTop;
  });
}

async function navigateAndWait(nodeId: string): Promise<void> {
  navigateToNode(nodeId);
  await waitUntil(() => store.get(displayedSurfaceNodeIdAtom) === nodeId, 5_000);
  const expectedScope = nodeIdToGridScope(nodeId);
  if (expectedScope) {
    await waitUntil(() => {
      const session = store.get(gridSessionAtom);
      return session.status === 'idle' && JSON.stringify(session.scope) === JSON.stringify(expectedScope);
    }, 15_000);
  }
}

export async function executeTutorialActions(actions: readonly TutorialAction[] = []): Promise<void> {
  for (const action of actions) {
    if (action.type === 'navigate') {
      await navigateAndWait(action.nodeId);
    } else if (action.type === 'select_first') {
      await waitUntil(() => store.get(gridItemsAtom).length > 0, 15_000);
      const first = store.get(gridItemsAtom)[0];
      if (first) store.set(gridSelectionAtom, { ...emptyGridSelection(), itemIds: new Set([first.root_id]), anchor: { kind: 'item', id: first.root_id } });
    } else if (action.type === 'select_first_kind') {
      await waitUntil(() => store.get(gridItemsAtom).some((entry) => entry.kind === action.kind), 15_000);
      const item = store.get(gridItemsAtom).find((entry) => entry.kind === action.kind);
      if (item) store.set(gridSelectionAtom, { ...emptyGridSelection(), itemIds: new Set([item.root_id]), anchor: { kind: 'item', id: item.root_id } });
    } else if (action.type === 'viewer') {
      if (action.mode === 'close') {
        store.set(viewerSessionAtom, null);
        store.set(quickLookSessionAtom, null);
      } else {
        const items = store.get(gridItemsAtom);
        const selected = [...store.get(gridSelectionAtom).itemIds][0];
        const currentIndex = Math.max(0, items.findIndex((entry) => entry.root_id === selected));
        const first = items[currentIndex];
        if (!first) continue;
        const session = { currentIndex, currentItemId: first.root_id };
        store.set(viewerSessionAtom, action.mode === 'detail' ? session : null);
        store.set(quickLookSessionAtom, action.mode === 'quick-look' ? session : null);
      }
    } else if (action.type === 'set_first_lifecycle') {
      const first = store.get(gridItemsAtom)[0];
      if (first) {
        lastLifecycleItemId = first.root_id;
        await invoke('items.set_lifecycle', { target: { kind: 'explicit', root_ids: [first.root_id] }, lifecycle: action.lifecycle });
        await waitUntil(() => !store.get(gridItemsAtom).some((entry) => entry.root_id === first.root_id), 5_000).catch(() => undefined);
      }
    } else if (action.type === 'restore_last_lifecycle') {
      if (lastLifecycleItemId != null) await invoke('items.set_lifecycle', { target: { kind: 'explicit', root_ids: [lastLifecycleItemId] }, lifecycle: action.lifecycle });
    } else if (action.type === 'set_tutorial_subscription_runs') {
      await setTutorialSubscriptionRuns(action.count);
    } else if (action.type === 'restore_rejected_item') {
      const first = store.get(gridItemsAtom)[0];
      if (first) await invoke('items.set_lifecycle', { target: { kind: 'explicit', root_ids: [first.root_id] }, lifecycle: 'active' });
    } else if (action.type === 'set_tutorial_folder_membership') {
      const selected = [...store.get(gridSelectionAtom).itemIds][0];
      if (selected == null) continue;
      const navigation = await invoke<{ folders: Array<{ folder_id: number }> }>('navigation.get', {});
      for (const folder of navigation.folders.slice(0, 2)) {
        await invoke('items.set_folder', { target: { kind: 'explicit', root_ids: [selected] }, folder_id: folder.folder_id, present: action.present });
      }
    } else if (action.type === 'set_tutorial_tag') {
      const selected = [...store.get(gridSelectionAtom).itemIds][0];
      if (selected != null) await invoke('items.apply_tags', { target: { kind: 'explicit', root_ids: [selected] }, tags: ['guided tour'], add: action.present });
    } else if (action.type === 'set_tutorial_collection_order') {
      const selected = store.get(viewerSessionAtom)?.currentItemId ?? [...store.get(gridSelectionAtom).itemIds][0];
      if (selected == null) continue;
      const details = await invoke<{ root: { kind: string }; media: Array<{ media_id: number }> }>('items.details', { root_id: selected });
      if (details.root.kind === 'collection' && details.media.length > 1) {
        const original = originalCollectionOrder.get(selected) ?? details.media.map((entry) => entry.media_id);
        originalCollectionOrder.set(selected, original);
        await invoke('items.reorder_collection', { collection_id: selected, media_ids: action.reversed ? [...original].reverse() : original });
      }
    }
  }
}

async function runTutorialSubscription(): Promise<void> {
  const list = await invoke<{ subscriptions: Array<{ subscription_id: number; name: string }> }>('subscriptions.list', {});
  const tutorial = list.subscriptions.find((entry) => entry.name === 'Leonardo da Vinci Archive');
  if (!tutorial) throw new Error('Tutorial subscription is missing');
  await invoke('subscriptions.run', { subscription_id: tutorial.subscription_id });
}

async function setTutorialSubscriptionRuns(count: 0 | 1 | 2): Promise<void> {
  const current = await tutorialSubscriptionRunCount();
  if (current > count || (count === 0 && current !== 0)) {
    await (window as any).picto.tutorial.reset();
    originalCollectionOrder.clear();
    lastLifecycleItemId = null;
    store.set(viewerSessionAtom, null);
    store.set(quickLookSessionAtom, null);
    store.set(gridSelectionAtom, emptyGridSelection());
    await navigateAndWait('system:active');
  }
  let completed = await tutorialSubscriptionRunCount();
  while (completed < count) {
    await runTutorialSubscription();
    await waitForSubscriptionIdle();
    completed = await tutorialSubscriptionRunCount();
  }
}

async function tutorialSubscriptionRunCount(): Promise<number> {
  const list = await invoke<{ subscriptions: Array<{ name: string; queries?: Array<{ successful_run_count: number }> }> }>('subscriptions.list', {});
  const tutorial = list.subscriptions.find((entry) => entry.name === 'Leonardo da Vinci Archive');
  if (!tutorial) throw new Error('Tutorial subscription is missing');
  return tutorial.queries?.[0]?.successful_run_count ?? 0;
}

async function waitForSubscriptionIdle(): Promise<void> {
  await waitUntil(async () => {
    const list = await invoke<{ subscriptions: Array<{ name: string; active_run_id: number | null; queries?: Array<{ successful_run_count: number }> }> }>('subscriptions.list', {});
    const tutorial = list.subscriptions.find((entry) => entry.name === 'Leonardo da Vinci Archive');
    return !!tutorial && tutorial.active_run_id == null && (tutorial.queries?.[0]?.successful_run_count ?? 0) > 0;
  }, 30_000);
}

export async function waitForTutorialCondition(condition?: TutorialCondition): Promise<void> {
  if (!condition) return;
  if (condition.type === 'grid_items') {
    await waitUntil(() => store.get(gridItemsAtom).length >= condition.minimum, 15_000);
  } else if (condition.type === 'subscription_idle') {
    await waitForSubscriptionIdle();
  } else {
    await waitUntil(async () => {
      await invoke('duplicates.scan', { distance_threshold: 12 });
      const candidates = await invoke<unknown[]>('duplicates.list', { limit: 10 });
      return candidates.length > 0;
    }, 30_000, 500);
  }
}

async function waitUntil(check: () => boolean | Promise<boolean>, timeoutMs: number, intervalMs = 50): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await check()) return;
    await new Promise((resolve) => window.setTimeout(resolve, intervalMs));
  }
  throw new Error('The guided tour timed out while preparing this step');
}

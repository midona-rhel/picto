/**
 * Navigation state — which sidebar scope is active, whether sidebar is visible.
 *
 * The grid area reads activeNodeIdAtom to know what to query.
 * Sidebar rows read it to show the active highlight.
 * The shell reads sidebarCollapsedAtom for layout.
 */

import { atom } from 'jotai';

/** The sidebar node ID that is currently active (e.g. "system:active", "folder:5"). */
export const activeNodeIdAtom = atom<string>('system:active');

export type SubscriptionsWorkspaceTab = 'subscriptions' | 'auth';
const SUBSCRIPTIONS_TAB_STORAGE_KEY = 'picto-subscriptions-workspace-tab';
const storedSubscriptionsTab = localStorage.getItem(SUBSCRIPTIONS_TAB_STORAGE_KEY);
const initialSubscriptionsTab: SubscriptionsWorkspaceTab =
  storedSubscriptionsTab === 'auth' ? 'auth' : 'subscriptions';

export const subscriptionsWorkspaceTabAtom = atom<SubscriptionsWorkspaceTab>(initialSubscriptionsTab);

/** Whether the sidebar is collapsed. Persisted to localStorage. */
const STORAGE_KEY = 'picto-sidebar-collapsed-panel';
const storedCollapsed = localStorage.getItem(STORAGE_KEY) === 'true';

export const sidebarCollapsedAtom = atom(storedCollapsed);

/** Toggle sidebar collapsed state and persist. */
export const toggleSidebarAtom = atom(null, (get, set) => {
  const next = !get(sidebarCollapsedAtom);
  set(sidebarCollapsedAtom, next);
  localStorage.setItem(STORAGE_KEY, String(next));
});

/** Whether the inspector is collapsed. Persisted to localStorage. */
const INSPECTOR_STORAGE_KEY = 'picto-inspector-collapsed';
const storedInspectorCollapsed = localStorage.getItem(INSPECTOR_STORAGE_KEY) === 'true';

export const inspectorCollapsedAtom = atom(storedInspectorCollapsed);

export const toggleInspectorAtom = atom(null, (get, set) => {
  const next = !get(inspectorCollapsedAtom);
  set(inspectorCollapsedAtom, next);
  localStorage.setItem(INSPECTOR_STORAGE_KEY, String(next));
});

export const setSubscriptionsWorkspaceTabAtom = atom(null, (_get, set, tab: SubscriptionsWorkspaceTab) => {
  set(subscriptionsWorkspaceTabAtom, tab);
  localStorage.setItem(SUBSCRIPTIONS_TAB_STORAGE_KEY, tab);
});

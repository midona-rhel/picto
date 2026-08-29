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

/** Node currently committed to the visible content surface during navigation transitions. */
export const displayedSurfaceNodeIdAtom = atom<string>('system:active');

/** When true, the next grid scope transition skips the fade-out phase. */
export const skipFadeOutAtom = atom(false);

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

/** Toggle both rails as one store transaction so layout observes one width. */
export const toggleBothPanelsAtom = atom(null, (get, set) => {
  const nextSidebar = !get(sidebarCollapsedAtom);
  const nextInspector = !get(inspectorCollapsedAtom);
  set(sidebarCollapsedAtom, nextSidebar);
  set(inspectorCollapsedAtom, nextInspector);
  localStorage.setItem(STORAGE_KEY, String(nextSidebar));
  localStorage.setItem(INSPECTOR_STORAGE_KEY, String(nextInspector));
});

export const showTreeGuidesAtom = atom(true);

export interface SidebarPreferences {
  showCounts: boolean;
  visibleSystemNodes: ReadonlySet<string>;
  showQuickAccess: boolean;
  showFolders: boolean;
  showSmartFolders: boolean;
  doubleClickAction: 'rename' | 'collapse';
}

export const sidebarPreferencesAtom = atom<SidebarPreferences>({
  showCounts: true,
  visibleSystemNodes: new Set([
    'system:active',
    'system:inbox',
    'system:recent_viewed',
    'system:uncategorized',
    'system:untagged',
    'system:tag_manager',
    'system:random',
    'system:subscriptions',
    'system:duplicates',
    'system:trash',
  ]),
  showQuickAccess: true,
  showFolders: true,
  showSmartFolders: true,
  doubleClickAction: 'collapse',
});

export interface ControlPreferences {
  gridWheelAction: 'scroll' | 'zoom';
  gridDoubleClickAction: 'detail' | 'external';
  gridMiddleClickAction: 'new_window' | 'none';
  spaceKeyAction: 'quick_look' | 'scroll';
}

export const controlPreferencesAtom = atom<ControlPreferences>({
  gridWheelAction: 'scroll',
  gridDoubleClickAction: 'detail',
  gridMiddleClickAction: 'new_window',
  spaceKeyAction: 'quick_look',
});

const INSPECTOR_WIDTH_STORAGE_KEY = 'picto-inspector-width';
export const INSPECTOR_MIN_WIDTH = 260;
export const INSPECTOR_MAX_WIDTH = 550;
const INSPECTOR_DEFAULT_WIDTH = 320;
const storedInspectorWidth = (() => {
  const v = parseInt(localStorage.getItem(INSPECTOR_WIDTH_STORAGE_KEY) ?? '', 10);
  return Number.isFinite(v) ? Math.min(INSPECTOR_MAX_WIDTH, Math.max(INSPECTOR_MIN_WIDTH, v)) : INSPECTOR_DEFAULT_WIDTH;
})();

export const inspectorWidthAtom = atom(storedInspectorWidth);
export const setInspectorWidthAtom = atom(null, (_get, set, width: number) => {
  const clamped = Math.min(INSPECTOR_MAX_WIDTH, Math.max(INSPECTOR_MIN_WIDTH, Math.round(width)));
  set(inspectorWidthAtom, clamped);
  localStorage.setItem(INSPECTOR_WIDTH_STORAGE_KEY, String(clamped));
});

const SIDEBAR_WIDTH_STORAGE_KEY = 'picto-sidebar-width';
export const SIDEBAR_MIN_WIDTH = 220;
export const SIDEBAR_MAX_WIDTH = 500;
const SIDEBAR_DEFAULT_WIDTH = 300;
const storedSidebarWidth = (() => {
  const value = parseInt(localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY) ?? '', 10);
  return Number.isFinite(value)
    ? Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, value))
    : SIDEBAR_DEFAULT_WIDTH;
})();

export const sidebarWidthAtom = atom(storedSidebarWidth);
export const setSidebarWidthAtom = atom(null, (_get, set, width: number) => {
  const clamped = Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, Math.round(width)));
  set(sidebarWidthAtom, clamped);
  localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(clamped));
});

/**
 * App shell — titlebar + sidebar + main content area.
 *
 * Titlebar is a drag region. Sidebar toggle, inspector toggle, and settings
 * buttons are right-aligned in the titlebar-left section.
 */

import { useCallback, useEffect, useRef } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconLayoutSidebar, IconSettings, IconChevronLeft, IconChevronRight, IconPin, IconPinFilled } from '@tabler/icons-react';
import { getSettings } from '../platform/api';
import { Sidebar } from '../features/sidebar/Sidebar';
import { GridScreen } from '../features/grid/GridScreen';
import { GridToolbar, ViewerToolbar } from '../features/grid/GridToolbar';
import { Inspector } from '../features/inspector/Inspector';
import { ModalLayer } from '../features/modals/ModalLayer';
import {
  sidebarCollapsedAtom, toggleSidebarAtom,
  inspectorCollapsedAtom, toggleInspectorAtom,
  inspectorWidthAtom, setInspectorWidthAtom,
  INSPECTOR_MIN_WIDTH, INSPECTOR_MAX_WIDTH,
  activeNodeIdAtom, parentNodeIdAtom,
  showTreeGuidesAtom,
} from '../state/navigation';
import { sidebarNodesAtom } from '../state/sidebar';
import { gridActiveAtom, gridScopeLabelAtom } from '../state/grid';
import { displayedScopeLabelAtom, displayedGridSnapshotAtom, inspectorPinnedAtom } from '../state/inspector';
import { viewerSessionAtom } from '../state/viewer';
import { startSidebarSettle } from '../runtime/sidebarSettle';
import { startGridSettle } from '../runtime/gridSettle';
import { startInspectorSync } from '../controllers/inspectorController';
import { zoomController } from '../controllers/zoomController';
import { canGoBackAtom, canGoForwardAtom, goBack, goForward, pushHistory } from '../state/navigationHistory';
import { getShortcut, matchesShortcutDef } from '../shared/lib/shortcuts';
import { KbdTooltip } from '../shared/ui/KbdTooltip';
import { WindowControls } from '../shared/ui/WindowControls';
import { listen } from '../platform/ipc';
import styles from './AppShell.module.css';

let cleanupFns: (() => void)[] = [];
function ensureSettle() {
  // Clean up previous listeners (HMR safety — old listeners are cancelled)
  for (const fn of cleanupFns) fn();
  cleanupFns = [
    startSidebarSettle(),
    startGridSettle(),
    startInspectorSync(),
  ];
}

function InspectorTitlebarActions() {
  const isPinned = useAtomValue(inspectorPinnedAtom);
  const setPinned = useSetAtom(inspectorPinnedAtom);
  return (
    <KbdTooltip label={isPinned ? 'Unpin' : 'Pin'}>
      <button
        className={`${styles.pinBtn} ${isPinned ? styles.pinBtnActive : ''}`}
        onClick={() => setPinned(!isPinned)}
        aria-label={isPinned ? 'Unpin Inspector' : 'Pin Inspector'}
      >
        {isPinned ? <IconPinFilled size={14} /> : <IconPin size={14} />}
      </button>
    </KbdTooltip>
  );
}

function openSettings() {
  (window as any).picto?.api?.invoke('open_settings_window')?.catch(() => {});
}

/** Build the full ancestor path for a sidebar node (folder or smart folder). */
function buildBreadcrumbPath(nodeId: string, nodes: { id: string; name: string; parent_id: string | null }[]): string[] {
  const path: string[] = [];
  let currentId: string | null = nodeId;
  const sectionRoots = new Set(['section:folders', 'section:smart_folders']);
  while (currentId) {
    const node = nodes.find((n) => n.id === currentId);
    if (!node || sectionRoots.has(node.id)) break;
    path.unshift(node.name);
    currentId = node.parent_id ?? null;
  }
  return path;
}

/** Scope title — shows full breadcrumb path for folders, smart folders, and collections. */
function ScopeTitle() {
  const gridActive = useAtomValue(gridActiveAtom);
  const frozenLabel = useAtomValue(displayedScopeLabelAtom);
  const liveLabel = useAtomValue(gridScopeLabelAtom);
  const label = gridActive ? (frozenLabel || liveLabel) : liveLabel;
  const snapshot = useAtomValue(displayedGridSnapshotAtom);
  const parentNodeId = useAtomValue(parentNodeIdAtom);
  const nodes = useAtomValue(sidebarNodesAtom);
  const setActiveNodeId = useSetAtom(activeNodeIdAtom);
  const setParentNodeId = useSetAtom(parentNodeIdAtom);

  if (!label) return null;

  const displayedNodeId = snapshot?.nodeId ?? '';

  // Collection breadcrumb: "Parent Scope / Collection Name"
  if (displayedNodeId.startsWith('collection:') && parentNodeId) {
    const parentPath = displayedNodeId.startsWith('collection:') && (parentNodeId.startsWith('folder:') || parentNodeId.startsWith('smart:'))
      ? buildBreadcrumbPath(parentNodeId, nodes)
      : [parentNodeId === 'system:active' ? 'All' : parentNodeId === 'system:inbox' ? 'Inbox' : parentNodeId === 'system:trash' ? 'Trash' : parentNodeId];

    return (
      <span className={styles.scopeTitle}>
        {parentPath.map((seg, i) => (
          <span key={i}>
            {i > 0 && <span style={{ opacity: 0.4, margin: '0 5px' }}>/</span>}
            <span style={{ opacity: 0.6 }}>{seg}</span>
          </span>
        ))}
        <span style={{ opacity: 0.4, margin: '0 5px' }}>/</span>
        {label}
      </span>
    );
  }

  // Folder / smart folder breadcrumb: full ancestor path
  if (displayedNodeId.startsWith('folder:') || displayedNodeId.startsWith('smart:')) {
    const path = buildBreadcrumbPath(displayedNodeId, nodes);
    if (path.length > 1) {
      return (
        <span className={styles.scopeTitle}>
          {path.map((seg, i) => (
            <span key={i}>
              {i > 0 && <span style={{ opacity: 0.4, margin: '0 5px' }}>/</span>}
              <span style={i < path.length - 1 ? { opacity: 0.6 } : undefined}>{seg}</span>
            </span>
          ))}
        </span>
      );
    }
  }

  return <span className={styles.scopeTitle}>{label}</span>;
}

export function AppShell() {
  const sidebarCollapsed = useAtomValue(sidebarCollapsedAtom);
  const inspectorCollapsed = useAtomValue(inspectorCollapsedAtom);
  const gridActive = useAtomValue(gridActiveAtom);
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const viewerSession = useAtomValue(viewerSessionAtom);
  const canBack = useAtomValue(canGoBackAtom);
  const canForward = useAtomValue(canGoForwardAtom);
  const inspectorWidth = useAtomValue(inspectorWidthAtom);
  const setInspectorWidth = useSetAtom(setInspectorWidthAtom);
  const toggleSidebar = useSetAtom(toggleSidebarAtom);
  const toggleInspector = useSetAtom(toggleInspectorAtom);
  const setActiveNodeId = useSetAtom(activeNodeIdAtom);

  const toggleBothPanels = () => { toggleSidebar(); toggleInspector(); };
  const isSubscriptionsWorkspace = activeNodeId === 'system:subscriptions';

  // ── Inspector resize drag ──
  const inspectorDragRef = useRef({ dragging: false, startX: 0, startWidth: 0 });
  const inspectorElRef = useRef<HTMLDivElement>(null);
  const onInspectorResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const el = inspectorElRef.current;
    const d = inspectorDragRef.current;
    d.dragging = true;
    d.startX = e.clientX;
    d.startWidth = el?.offsetWidth ?? inspectorWidth;
    el?.classList.add(styles.inspectorDragging);

    const onMove = (ev: MouseEvent) => {
      if (!d.dragging) return;
      const delta = d.startX - ev.clientX;
      const next = Math.min(INSPECTOR_MAX_WIDTH, Math.max(INSPECTOR_MIN_WIDTH, Math.round(d.startWidth + delta)));
      if (el) el.style.width = `${next}px`;
      document.documentElement.style.setProperty('--inspector-width', `${next}px`);
    };
    const onUp = () => {
      if (!d.dragging) return;
      d.dragging = false;
      el?.classList.remove(styles.inspectorDragging);
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      setInspectorWidth(el?.offsetWidth ?? d.startWidth);
    };
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  }, [inspectorWidth, setInspectorWidth]);

  // Keep --inspector-width CSS variable in sync
  const setShowTreeGuides = useSetAtom(showTreeGuidesAtom);

  useEffect(() => {
    ensureSettle();

    const applyTheme = (theme: string) => {
      const lightThemes = new Set(['light', 'lightgray']);
      const resolved = theme === 'auto'
        ? (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
        : theme;
      document.documentElement.dataset.theme = theme === 'auto' ? '' : theme;
      document.documentElement.dataset.mantineColorScheme = lightThemes.has(resolved) ? 'light' : 'dark';
      document.documentElement.style.colorScheme = lightThemes.has(resolved) ? 'light' : 'dark';
      localStorage.setItem('picto-theme', theme);
    };

    const loadAppSettings = () => {
      getSettings().then((s) => {
        setShowTreeGuides(s.showTreeGuides ?? true);
        if (s.colorScheme) applyTheme(s.colorScheme);
      }).catch(() => {});
    };

    loadAppSettings();

    // Reload settings when backend emits view_prefs_changed (e.g. settings window saved)
    let unlistenSettings: (() => void) | undefined;
    let unlistenOsTheme: (() => void) | undefined;
    void import('../platform/ipc').then(({ listen }) => {
      listen<{ changes?: { view_prefs_changed?: boolean } }>('runtime/state_changed', (event) => {
        if (event.payload.changes?.view_prefs_changed) loadAppSettings();
      }).then((fn) => { unlistenSettings = fn; });

      // Auto theme: OS dark/light mode changed
      listen<{ isDark: boolean }>('picto:os-theme-changed', (event) => {
        const currentTheme = localStorage.getItem('picto-theme');
        if (currentTheme !== 'auto') return;
        const resolved = event.payload.isDark ? 'dark' : 'light';
        const lightThemes = new Set(['light', 'lightgray']);
        document.documentElement.dataset.theme = '';
        document.documentElement.dataset.mantineColorScheme = lightThemes.has(resolved) ? 'light' : 'dark';
        document.documentElement.style.colorScheme = lightThemes.has(resolved) ? 'light' : 'dark';
      }).then((fn) => { unlistenOsTheme = fn; });
    });
    return () => { unlistenSettings?.(); unlistenOsTheme?.(); };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    const scopeMap: Record<string, string> = {
      images: 'system:active',
      review: 'system:inbox',
      untagged: 'system:untagged',
      trash: 'system:trash',
      duplicates: 'system:duplicates',
      subscriptions: 'system:subscriptions',
    };

    void listen<string>('menu:navigate', (event) => {
      if (cancelled) return;
      const nextNodeId = scopeMap[event.payload];
      if (!nextNodeId) return;
      setActiveNodeId(nextNodeId);
      pushHistory(nextNodeId);
    }).then((dispose) => {
      if (cancelled) {
        dispose();
        return;
      }
      unlisten = dispose;
    }).catch((err) => {
      console.error('Failed to subscribe to menu navigation', err);
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [setActiveNodeId]);

  // App-wide keyboard shortcuts — uses registry defs so keys2 (EU alternatives) work
  useEffect(() => {
    const defs = {
      sidebar:    getShortcut('view.toggleSidebar')!,
      inspector:  getShortcut('view.toggleInspector')!,
      panels:     getShortcut('view.toggleBothPanels')!,
      settings:   getShortcut('file.settings')!,
      back:       getShortcut('nav.back')!,
      forward:    getShortcut('nav.forward')!,
    };

    function handleKeyDown(e: KeyboardEvent) {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      if (matchesShortcutDef(e, defs.sidebar))   { e.preventDefault(); toggleSidebar(); return; }
      if (matchesShortcutDef(e, defs.settings))   { e.preventDefault(); openSettings(); return; }
      if (matchesShortcutDef(e, defs.inspector))  { e.preventDefault(); toggleInspector(); return; }
      if (matchesShortcutDef(e, defs.back))       { e.preventDefault(); goBack(); return; }
      if (matchesShortcutDef(e, defs.forward))    { e.preventDefault(); goForward(); return; }
      if (matchesShortcutDef(e, defs.panels))     { e.preventDefault(); toggleBothPanels(); return; }

      // Zoom: Mod+= / Mod++ / Mod+- / Mod+0
      if ((e.metaKey || e.ctrlKey) && (e.key === '=' || e.key === '+')) { e.preventDefault(); zoomController.zoomIn(); return; }
      if ((e.metaKey || e.ctrlKey) && e.key === '-') { e.preventDefault(); zoomController.zoomOut(); return; }
      if ((e.metaKey || e.ctrlKey) && e.key === '0') { e.preventDefault(); zoomController.resetZoom(); return; }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [toggleSidebar, toggleInspector, toggleBothPanels]);

  const showInspector = gridActive && !inspectorCollapsed && !isSubscriptionsWorkspace;

  useEffect(() => {
    document.documentElement.style.setProperty(
      '--inspector-width',
      showInspector ? `${inspectorWidth}px` : '0px',
    );
  }, [showInspector, inspectorWidth]);

  return (
    <div className={styles.shell}>
      <div className={styles.titlebar}>
        <div className={sidebarCollapsed ? styles.titlebarLeftCollapsed : styles.titlebarLeft}>
          <div className={styles.titlebarActions}>
            <KbdTooltip label="Settings" shortcut="Mod+,">
              <button className={styles.toggleBtn} onClick={openSettings}>
                <IconSettings size={16} stroke={1.5} />
              </button>
            </KbdTooltip>
            <KbdTooltip label="Toggle Panels" shortcut="Tab">
              <button className={styles.toggleBtn} onClick={toggleBothPanels}>
                <IconLayoutSidebar size={18} stroke={1.5} />
              </button>
            </KbdTooltip>
          </div>
        </div>
        <div className={styles.titlebarCenter}>
          {viewerSession ? (
            <ViewerToolbar />
          ) : (
            <>
              <button className={`${styles.navBtn} ${!canBack ? styles.navBtnDisabled : ''}`} onClick={canBack ? goBack : undefined} title="Back (⌘[)">
                <IconChevronLeft size={16} stroke={1.5} />
              </button>
              <button className={`${styles.navBtn} ${!canForward ? styles.navBtnDisabled : ''}`} onClick={canForward ? goForward : undefined} title="Forward (⌘])">
                <IconChevronRight size={16} stroke={1.5} />
              </button>
              <ScopeTitle />
              {gridActive && !isSubscriptionsWorkspace ? (
                <GridToolbar />
              ) : null}
              {!showInspector && <WindowControls />}
            </>
          )}
        </div>
      </div>

      <div className={styles.body}>
        {!sidebarCollapsed && (
          <div className={styles.sidebar}>
            <Sidebar />
          </div>
        )}
        <div className={styles.main}>
          <GridScreen />
        </div>
      </div>
      {showInspector && (
        <div ref={inspectorElRef} className={styles.inspector} style={{ width: inspectorWidth }}>
          <div className={styles.inspectorResizeHandle} onMouseDown={onInspectorResizeStart} />
          <div className={styles.inspectorTitlebar}>
            <InspectorTitlebarActions />
            <WindowControls />
          </div>
          <Inspector />
        </div>
      )}
      <ModalLayer />
    </div>
  );
}

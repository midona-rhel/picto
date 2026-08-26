/**
 * App shell — titlebar + sidebar + main content area.
 *
 * Titlebar is a drag region. Sidebar toggle, inspector toggle, and settings
 * buttons are right-aligned in the titlebar-left section.
 */

import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { isNativeDragPending as isNativeDragPendingFn } from '../features/grid/dragState';
import { useAtom, useAtomValue, useSetAtom } from 'jotai';
import { IconSettings, IconPin, IconPinFilled } from '@tabler/icons-react';
import { ToolbarHistoryIcon, ToolbarPanelIcon } from '../shared/ui/icons/toolbar-icons';
import { Sidebar } from '../features/sidebar/Sidebar';
import { WorkspaceSurface } from '../features/workspace/WorkspaceSurface';
import { GridToolbar, ViewerToolbar } from '../features/grid/GridToolbar';
import { TagsToolbar } from '../features/tags/TagManagerScreen';
import { DuplicatesToolbar } from '../features/duplicates/DuplicatesScreen';
import { Inspector } from '../features/inspector/Inspector';
import { ModalLayer } from '../features/modals/ModalLayer';
import { TagSelectPanel } from '../features/tags/TagSelectPanel';
import { FolderPickerPanel } from '../features/folders/FolderPickerPanel';
import { AiTaggerPanel } from '../features/ai-tagger/AiTaggerPanel';
import { DiagnosticsPanel } from '../features/diagnostics/DiagnosticsPanel';
import { listen } from '../platform/ipc';
import {
  sidebarCollapsedAtom, toggleSidebarAtom,
  inspectorCollapsedAtom, toggleInspectorAtom, toggleBothPanelsAtom,
  inspectorWidthAtom, setInspectorWidthAtom,
  INSPECTOR_MIN_WIDTH, INSPECTOR_MAX_WIDTH,
  displayedSurfaceNodeIdAtom,
  showTreeGuidesAtom,
  sidebarPreferencesAtom,
  controlPreferencesAtom,
} from '../state/navigation';
import { sidebarNodesAtom } from '../state/sidebar';
import { gridActiveAtom, gridDefaultSpacingAtom, gridScopeLabelAtom, gridTransitionPhaseAtom } from '../state/grid';
import { displayedScopeLabelAtom, displayedGridSnapshotAtom, inspectorPinnedAtom } from '../state/inspector';
import { viewerExitTransitionAtom, viewerSessionAtom } from '../state/viewer';
import { startAppRuntime } from '../runtime/appRuntime';
import { registerAppSettingsReload } from '../runtime/appSettingsSettle';
import { useShortcutScope } from '../shared/hooks/useShortcutScope';
import { zoomController } from '../controllers/zoomController';
import { gridController } from '../controllers/gridController';
import { canGoBackAtom, canGoForwardAtom, goBack, goForward, navigateToNode, navigateWithGridFilters, pushSubscriptionsHistory } from '../state/navigationHistory';
import { subscriptionsSelectionAtom, subscriptionsWorkspaceSnapshotAtom } from '../state/subscriptionsWorkspace';
import { formatKeysDisplay, getShortcut, matchesShortcutDef } from '../shared/lib/shortcuts';
import { KbdTooltip } from '../shared/ui/KbdTooltip';
import { ContextMenu, useContextMenu } from '../shared/ui/ContextMenu/ContextMenu';
import type { MenuEntry } from '../shared/ui/ContextMenu/ContextMenu';
import { TitlebarControlButton } from '../shared/ui/TitlebarControls';
import { WindowControls } from '../shared/ui/WindowControls';
import { ApplicationMenuButton } from '../shared/ui/ApplicationMenuButton/ApplicationMenuButton';
import { appController } from '../controllers/appController';
import { settingsController } from '../controllers/settingsController';
import { applyPreviewPreferences } from '../features/viewer/usePreviewPreferences';
import { configureNotificationPopups } from '../shared/lib/notifications';
import { aiTaggerPortalAtom, folderPickerPortalAtom, tagSelectPortalAtom } from '../state/portals';
import { createEmptyItemFilters, itemFiltersEqual } from '../shared/lib/itemFilters';
import styles from './AppShell.module.css';

const isMacPlatform = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

type PanelPresencePhase = 'shown' | 'entering' | 'exiting';
type PanelPresenceMotion = 'slide' | 'fade';

function usePanelPresence(visible: boolean, duration: number, motion: PanelPresenceMotion = 'slide') {
  const [presence, setPresence] = useState<{
    rendered: boolean;
    phase: PanelPresencePhase;
    motion: PanelPresenceMotion;
  }>(() => ({
    rendered: visible,
    phase: 'shown',
    motion,
  }));
  const previousVisible = useRef(visible);

  useEffect(() => {
    if (previousVisible.current === visible) return;
    previousVisible.current = visible;

    setPresence({ rendered: true, phase: visible ? 'entering' : 'exiting', motion });
    const timer = window.setTimeout(() => {
      setPresence(visible
        ? { rendered: true, phase: 'shown', motion }
        : { rendered: false, phase: 'shown', motion });
    }, duration);
    return () => window.clearTimeout(timer);
  }, [duration, motion, visible]);

  return presence;
}

export function buildPanelVisibilityContextEntries({
  toggleAll,
  toggleSidebar,
  toggleInspector,
}: {
  toggleAll: () => void;
  toggleSidebar: () => void;
  toggleInspector: () => void;
}): MenuEntry[] {
  return [
    {
      label: 'Toggle All Panels',
      shortcut: formatKeysDisplay(getShortcut('view.toggleBothPanels')!.keys),
      action: toggleAll,
      keepOpen: true,
    },
    {
      label: 'Toggle Sidebar',
      shortcut: formatKeysDisplay(getShortcut('view.toggleSidebar')!.keys),
      action: toggleSidebar,
      keepOpen: true,
    },
    {
      label: 'Toggle Inspector',
      shortcut: formatKeysDisplay(getShortcut('view.toggleInspector')!.keys),
      action: toggleInspector,
      keepOpen: true,
    },
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
  void appController.openSettingsWindow().catch(() => {});
}

/** Build the full ancestor path for a sidebar node (folder or smart folder). */
function buildBreadcrumbPath(
  nodeId: string,
  nodes: { id: string; name: string; parent_id: string | null }[],
): { id: string; name: string }[] {
  const path: { id: string; name: string }[] = [];
  let currentId: string | null = nodeId;
  const sectionRoots = new Set(['section:folders', 'section:smart_folders']);
  while (currentId) {
    const node = nodes.find((n) => n.id === currentId);
    if (!node || sectionRoots.has(node.id)) break;
    path.unshift({ id: node.id, name: node.name });
    currentId = node.parent_id ?? null;
  }
  return path;
}

/** Scope title — shows the full breadcrumb path for folders and smart folders. */
function ScopeTitle() {
  const gridActive = useAtomValue(gridActiveAtom);
  const transitionPhase = useAtomValue(gridTransitionPhaseAtom);
  const frozenLabel = useAtomValue(displayedScopeLabelAtom);
  const liveLabel = useAtomValue(gridScopeLabelAtom);
  const label = gridActive ? (frozenLabel || liveLabel) : liveLabel;
  const snapshot = useAtomValue(displayedGridSnapshotAtom);
  const nodes = useAtomValue(sidebarNodesAtom);
  const displayedSurfaceNodeId = useAtomValue(displayedSurfaceNodeIdAtom);
  const [subsSelection, setSubsSelection] = useAtom(subscriptionsSelectionAtom);
  const subsSnapshot = useAtomValue(subscriptionsWorkspaceSnapshotAtom);
  const titleProps = {
    className: styles.scopeTitle,
    'data-transition-phase': transitionPhase,
  } as const;

  // Subscription breadcrumb: Subscriptions [/ Subscription].
  if (displayedSurfaceNodeId === 'system:subscriptions') {
    const selectedSub =
      subsSelection?.kind === 'subscription'
        ? subsSnapshot?.subscriptions.find((sub) => sub.id === subsSelection.id) ?? null
        : null;
    const leafName = selectedSub?.name ?? null;
    if (!leafName) return <span {...titleProps}>Subscriptions</span>;

    const crumbSeparator = <span style={{ opacity: 0.4, margin: '0 5px' }}>/</span>;
    return (
      <span {...titleProps}>
        <button
          type="button"
          className={styles.scopeCrumbLink}
          onClick={() => {
            setSubsSelection(null);
            pushSubscriptionsHistory(null);
          }}
        >
          Subscriptions
        </button>
        {crumbSeparator}
        {leafName}
      </span>
    );
  }

  if (displayedSurfaceNodeId === 'system:duplicates') {
    return <span {...titleProps}>Duplicates</span>;
  }

  const displayedNodeId = snapshot?.nodeId ?? '';
  const showsSearchResults = snapshot != null && (
    snapshot.searchText.length > 0
    || !itemFiltersEqual(snapshot.filters, createEmptyItemFilters())
  );
  const searchResultsLabel = showsSearchResults
    ? `Search results${snapshot.totalCount == null ? '' : ` · ${snapshot.totalCount.toLocaleString()}`}`
    : '';
  const crumbSeparator = <span style={{ opacity: 0.4, margin: '0 5px' }}>/</span>;
  const clearSearchResults = (nodeId: string) => {
    gridController.setSearchText('');
    navigateWithGridFilters(nodeId, createEmptyItemFilters());
  };

  if (displayedSurfaceNodeId === 'system:tag_manager') {
    if (!showsSearchResults) return <span {...titleProps}>Tags</span>;
    return (
      <span {...titleProps}>
        <button
          type="button"
          className={styles.scopeCrumbLink}
          onClick={() => {
            gridController.setSearchText('');
            navigateToNode('system:tag_manager');
          }}
        >
          Tags
        </button>
        {crumbSeparator}
        {searchResultsLabel}
      </span>
    );
  }

  if (!label) return null;

  // Folder / smart folder breadcrumb: full ancestor path
  if (displayedNodeId.startsWith('folder:') || displayedNodeId.startsWith('smart:')) {
    const path = buildBreadcrumbPath(displayedNodeId, nodes);
    if (path.length > 0) {
      return (
        <span {...titleProps}>
          {path.map((seg, i) => (
            <span key={seg.id}>
              {i > 0 && <span style={{ opacity: 0.4, margin: '0 5px' }}>/</span>}
              {i < path.length - 1 || showsSearchResults ? (
                <button
                  type="button"
                  className={styles.scopeCrumbLink}
                  onClick={() => i === path.length - 1
                    ? clearSearchResults(seg.id)
                    : navigateToNode(seg.id)}
                >
                  {seg.name}
                </button>
              ) : (
                <span>{seg.name}</span>
              )}
            </span>
          ))}
          {showsSearchResults && <>{crumbSeparator}{searchResultsLabel}</>}
        </span>
      );
    }
  }

  if (showsSearchResults) {
    return (
      <span {...titleProps}>
        <button
          type="button"
          className={styles.scopeCrumbLink}
          onClick={() => clearSearchResults(displayedNodeId)}
        >
          {label}
        </button>
        {crumbSeparator}
        {searchResultsLabel}
      </span>
    );
  }

  return <span {...titleProps}>{label}</span>;
}

export function AppShell() {
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const sidebarCollapsed = useAtomValue(sidebarCollapsedAtom);
  const inspectorCollapsed = useAtomValue(inspectorCollapsedAtom);
  const gridActive = useAtomValue(gridActiveAtom);
  const displayedSurfaceNodeId = useAtomValue(displayedSurfaceNodeIdAtom);
  const transitionPhase = useAtomValue(gridTransitionPhaseAtom);
  const viewerSession = useAtomValue(viewerSessionAtom);
  const viewerExitTransition = useAtomValue(viewerExitTransitionAtom);
  const canBack = useAtomValue(canGoBackAtom);
  const canForward = useAtomValue(canGoForwardAtom);
  const inspectorWidth = useAtomValue(inspectorWidthAtom);
  const setInspectorWidth = useSetAtom(setInspectorWidthAtom);
  const toggleSidebar = useSetAtom(toggleSidebarAtom);
  const toggleInspector = useSetAtom(toggleInspectorAtom);
  const toggleBothPanels = useSetAtom(toggleBothPanelsAtom);
  const panelMenu = useContextMenu();
  const setTagSelectPortal = useSetAtom(tagSelectPortalAtom);
  const setFolderPickerPortal = useSetAtom(folderPickerPortalAtom);
  const setAiTaggerPortal = useSetAtom(aiTaggerPortalAtom);
  const isSubscriptionsWorkspace = displayedSurfaceNodeId === 'system:subscriptions';
  const reserveInspectorTitlebar = gridActive && !isSubscriptionsWorkspace;
  const titlebarLeftClass = sidebarCollapsed
    ? (isMacPlatform ? styles.titlebarLeftPanelHiddenMac : styles.titlebarLeftPanelHidden)
    : (isMacPlatform ? styles.titlebarLeftMac : styles.titlebarLeft);

  useEffect(() => {
    setTagSelectPortal({ open: false, anchor: null });
    setFolderPickerPortal({ open: false, anchor: null });
    setAiTaggerPortal({ open: false, anchor: null });
  }, [displayedSurfaceNodeId, setTagSelectPortal, setFolderPickerPortal, setAiTaggerPortal]);

  useEffect(() => {
    let stopped = false;
    let dispose: (() => void) | undefined;
    void listen('menu:toggle-diagnostics', () => setDiagnosticsOpen((open) => !open))
      .then((unlisten) => {
        if (stopped) unlisten();
        else dispose = unlisten;
      });
    return () => {
      stopped = true;
      dispose?.();
    };
  }, []);

  useEffect(() => {
    const openDiagnostics = () => setDiagnosticsOpen(true);
    window.addEventListener('picto:open-diagnostics', openDiagnostics);
    return () => window.removeEventListener('picto:open-diagnostics', openDiagnostics);
  }, []);


  // ── Inspector resize drag ──
  const inspectorDragRef = useRef({ dragging: false, startX: 0, startWidth: 0 });
  const shellRef = useRef<HTMLDivElement>(null);
  const inspectorElRef = useRef<HTMLDivElement>(null);
  const onInspectorResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const el = inspectorElRef.current;
    const d = inspectorDragRef.current;
    d.dragging = true;
    d.startX = e.clientX;
    d.startWidth = el?.offsetWidth ?? inspectorWidth;
    el?.classList.add(styles.inspectorDragging);

    // Coalesce mousemove bursts to one layout write per frame.
    let pendingWidth = -1;
    let rafId = 0;
    const flush = () => {
      rafId = 0;
      if (pendingWidth < 0) return;
      if (el) el.style.width = `${pendingWidth}px`;
      shellRef.current?.style.setProperty('--inspector-width', `${pendingWidth}px`);
      pendingWidth = -1;
    };
    const onMove = (ev: MouseEvent) => {
      if (!d.dragging) return;
      const delta = d.startX - ev.clientX;
      pendingWidth = Math.min(INSPECTOR_MAX_WIDTH, Math.max(INSPECTOR_MIN_WIDTH, Math.round(d.startWidth + delta)));
      if (!rafId) rafId = requestAnimationFrame(flush);
    };
    const onUp = () => {
      if (!d.dragging) return;
      d.dragging = false;
      if (rafId) cancelAnimationFrame(rafId);
      flush();
      el?.classList.remove(styles.inspectorDragging);
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      setInspectorWidth(el?.offsetWidth ?? d.startWidth);
    };
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  }, [inspectorWidth, setInspectorWidth]);

  const setShowTreeGuides = useSetAtom(showTreeGuidesAtom);
  const setSidebarPreferences = useSetAtom(sidebarPreferencesAtom);
  const setControlPreferences = useSetAtom(controlPreferencesAtom);
  const setGridSpacing = useSetAtom(gridDefaultSpacingAtom);

  useEffect(() => {
    const stopRuntime = startAppRuntime();

    const loadAppSettings = () => {
      settingsController.getSettings().then((s) => {
        applyPreviewPreferences(s);
        configureNotificationPopups({
          enabled: s.notificationPopupsEnabled,
          tones: s.notificationPopupTones,
        });
        setShowTreeGuides(s.showTreeGuides ?? true);
        setSidebarPreferences({
          showCounts: s.showSidebarCounts,
          visibleSystemNodes: new Set([
            'system:active',
            s.showSidebarInbox && 'system:inbox',
            s.showSidebarRecentlyViewed && 'system:recent_viewed',
            s.showSidebarUncategorized && 'system:uncategorized',
            s.showSidebarUntagged && 'system:untagged',
            s.showSidebarTagManager && 'system:tag_manager',
            s.showSidebarRandom && 'system:random',
            s.showSidebarSubscriptions && 'system:subscriptions',
            s.showSidebarDuplicates && 'system:duplicates',
            'system:trash',
          ].filter((nodeId): nodeId is string => Boolean(nodeId))),
          showQuickAccess: s.showSidebarQuickAccess,
          showFolders: s.showSidebarFolders,
          showSmartFolders: s.showSidebarSmartFolders,
          doubleClickAction: s.sidebarDoubleClickAction,
        });
        setControlPreferences({
          gridWheelAction: s.gridWheelAction,
          gridDoubleClickAction: s.gridDoubleClickAction,
          gridMiddleClickAction: s.gridMiddleClickAction,
          spaceKeyAction: s.spaceKeyAction,
        });
        setGridSpacing(s.gridSpacing);
      }).catch(() => {});
    };

    loadAppSettings();

    const unregisterSettingsReload = registerAppSettingsReload(loadAppSettings);
    return () => {
      stopRuntime();
      unregisterSettingsReload();
    };
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

    void appController.subscribeMenuNavigate((destination) => {
      if (cancelled) return;
      const nextNodeId = scopeMap[destination];
      if (!nextNodeId) return;
      navigateToNode(nextNodeId);
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
  }, []);

  // ── Re-import guard — prevent dropping files back into the app during a native drag ──
  useEffect(() => {
    const handler = (e: DragEvent) => {
      if (isNativeDragPendingFn()) {
        e.preventDefault();
        e.stopPropagation();
        if (e.dataTransfer) e.dataTransfer.dropEffect = 'none';
      }
    };
    window.addEventListener('dragenter', handler, true);
    window.addEventListener('dragover', handler, true);
    window.addEventListener('drop', handler, true);
    return () => {
      window.removeEventListener('dragenter', handler, true);
      window.removeEventListener('dragover', handler, true);
      window.removeEventListener('drop', handler, true);
    };
  }, []);

  // App-wide keyboard shortcuts — uses registry defs so keys2 (EU alternatives) work
  useShortcutScope((e) => {
    const defs = {
      sidebar:    getShortcut('view.toggleSidebar')!,
      inspector:  getShortcut('view.toggleInspector')!,
      panels:     getShortcut('view.toggleBothPanels')!,
      settings:   getShortcut('file.settings')!,
      back:       getShortcut('nav.back')!,
      forward:    getShortcut('nav.forward')!,
    };

    // Suppress browser Tab focus navigation (but let it fall through to shortcut matching).
    if (e.key === 'Tab') e.preventDefault();

    if (matchesShortcutDef(e, defs.sidebar))   { e.preventDefault(); toggleSidebar(); return; }
    if (matchesShortcutDef(e, defs.settings))  { e.preventDefault(); openSettings(); return; }
    if (matchesShortcutDef(e, defs.inspector)) { e.preventDefault(); toggleInspector(); return; }
    if (matchesShortcutDef(e, defs.back))      { e.preventDefault(); goBack(); return; }
    if (matchesShortcutDef(e, defs.forward))   { e.preventDefault(); goForward(); return; }
    if (matchesShortcutDef(e, defs.panels))    { e.preventDefault(); toggleBothPanels(); return; }

    // Zoom: Mod+= / Mod++ / Mod+- / Mod+0
    if ((e.metaKey || e.ctrlKey) && (e.key === '=' || e.key === '+')) { e.preventDefault(); zoomController.zoomIn(); return; }
    if ((e.metaKey || e.ctrlKey) && e.key === '-') { e.preventDefault(); zoomController.zoomOut(); return; }
    if ((e.metaKey || e.ctrlKey) && e.key === '0') { e.preventDefault(); zoomController.resetZoom(); }
  });

  const inspectorAvailable = gridActive && !isSubscriptionsWorkspace;
  const showInspector = inspectorAvailable && !inspectorCollapsed;
  const previousInspectorAvailable = useRef(inspectorAvailable);
  const inspectorMotion = previousInspectorAvailable.current === inspectorAvailable ? 'slide' : 'fade';
  const sidebarPresence = usePanelPresence(!sidebarCollapsed, 50);
  const inspectorPresence = usePanelPresence(showInspector, inspectorMotion === 'fade' ? 170 : 100, inspectorMotion);

  useEffect(() => {
    previousInspectorAvailable.current = inspectorAvailable;
  }, [inspectorAvailable]);

  return (
    <div
      ref={shellRef}
      className={`${styles.shell} ${isMacPlatform ? styles.shellMac : ''}`}
      style={{
        '--inspector-width': showInspector ? `${inspectorWidth}px` : '0px',
        '--sidebar-body-width': sidebarCollapsed ? '0px' : 'var(--sidebar-width)',
        '--titlebar-inspector-width': reserveInspectorTitlebar ? `${inspectorWidth}px` : '0px',
      } as CSSProperties}
    >
      <div className={styles.titlebar} data-window-drag-region="">
        <div className={titlebarLeftClass} data-help-region="sidebar">
          <ApplicationMenuButton />
          <div className={styles.titlebarActions}>
            <KbdTooltip label="Settings" shortcutId="file.settings">
              <button className={styles.toggleBtn} onClick={openSettings}>
                <IconSettings size={16} stroke={1.5} />
              </button>
            </KbdTooltip>
            <KbdTooltip label="Toggle Panels" shortcutId="view.toggleBothPanels">
              <button
                className={styles.toggleBtn}
                data-help-id="panel-controls"
                onClick={toggleBothPanels}
                onContextMenu={(event) => panelMenu.open(event, buildPanelVisibilityContextEntries({
                  toggleAll: toggleBothPanels,
                  toggleSidebar,
                  toggleInspector,
                }), { showSearch: false })}
              >
                <ToolbarPanelIcon size={14} />
              </button>
            </KbdTooltip>
          </div>
        </div>
        <div className={styles.titlebarCenter} data-help-id="toolbar">
          <div
            className={styles.titlebarContent}
            data-viewer-exit-transition={viewerExitTransition || undefined}
            data-transition-phase={transitionPhase}
          >
            {viewerSession ? (
              <ViewerToolbar />
            ) : (
              <>
                <KbdTooltip label="Back" shortcutId="nav.back">
                  <TitlebarControlButton disabled={!canBack} onClick={canBack ? goBack : undefined}>
                    <ToolbarHistoryIcon direction="back" />
                  </TitlebarControlButton>
                </KbdTooltip>
                <KbdTooltip label="Forward" shortcutId="nav.forward">
                  <TitlebarControlButton disabled={!canForward} onClick={canForward ? goForward : undefined}>
                    <ToolbarHistoryIcon direction="forward" />
                  </TitlebarControlButton>
                </KbdTooltip>
                <ScopeTitle />
                {gridActive && !isSubscriptionsWorkspace ? (
                  <GridToolbar />
                ) : null}
                {displayedSurfaceNodeId === 'system:duplicates' ? <DuplicatesToolbar /> : null}
                {displayedSurfaceNodeId === 'system:tag_manager' && !gridActive ? <TagsToolbar /> : null}
                {!reserveInspectorTitlebar && <WindowControls />}
              </>
            )}
          </div>
        </div>
      </div>
      {panelMenu.state && (
        <ContextMenu
          entries={panelMenu.state.entries}
          position={panelMenu.state.position}
          onClose={panelMenu.close}
          showSearch={panelMenu.state.showSearch}
        />
      )}

      <div className={styles.body}>
        {sidebarPresence.rendered && (
          <div
            className={styles.sidebar}
            data-help-id="sidebar"
            data-presence={sidebarPresence.phase}
            data-motion={sidebarPresence.motion}
          >
            <Sidebar />
          </div>
        )}
        <div
          className={styles.main}
          data-help-id="workspace"
        >
          <WorkspaceSurface />
        </div>
      </div>
      {reserveInspectorTitlebar && !inspectorPresence.rendered && (
        <div className={styles.titlebarInspectorHidden} style={{ width: inspectorWidth }}>
          <WindowControls />
        </div>
      )}
      {inspectorPresence.rendered && (
        <div
          ref={inspectorElRef}
          className={styles.inspector}
          data-help-id="inspector"
          data-presence={inspectorPresence.phase}
          data-motion={inspectorPresence.motion}
          data-transition-phase={transitionPhase}
          style={{ width: inspectorWidth }}
        >
          <div className={styles.inspectorResizeHandle} onMouseDown={onInspectorResizeStart} />
          <div className={styles.inspectorTitlebar}>
            <InspectorTitlebarActions />
            <WindowControls />
          </div>
          <Inspector />
        </div>
      )}
      <ModalLayer />
      <TagSelectPanel />
      <FolderPickerPanel />
      <AiTaggerPanel />
      {diagnosticsOpen ? <DiagnosticsPanel onClose={() => setDiagnosticsOpen(false)} /> : null}
    </div>
  );
}

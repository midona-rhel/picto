/**
 * App shell — titlebar + sidebar + main content area.
 *
 * Titlebar is a drag region. Sidebar toggle, inspector toggle, and settings
 * buttons are right-aligned in the titlebar-left section.
 */

import { useEffect } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconAntennaBars5, IconLock, IconLayoutSidebar, IconSettings, IconChevronLeft, IconChevronRight, IconPin, IconPinFilled } from '@tabler/icons-react';
import { Sidebar } from '../features/sidebar/Sidebar';
import { GridScreen } from '../features/grid/GridScreen';
import { GridToolbar, ViewerToolbar } from '../features/grid/GridToolbar';
import { Inspector } from '../features/inspector/Inspector';
import {
  sidebarCollapsedAtom, toggleSidebarAtom,
  inspectorCollapsedAtom, toggleInspectorAtom,
  activeNodeIdAtom,
  subscriptionsWorkspaceTabAtom,
  setSubscriptionsWorkspaceTabAtom,
} from '../state/navigation';
import { gridActiveAtom, gridScopeLabelAtom } from '../state/grid';
import { displayedScopeLabelAtom, inspectorPinnedAtom } from '../state/inspector';
import { viewerSessionAtom } from '../state/viewer';
import { startSidebarSettle } from '../runtime/sidebarSettle';
import { startGridSettle } from '../runtime/gridSettle';
import { startInspectorSync } from '../controllers/inspectorController';
import { zoomController } from '../controllers/zoomController';
import { canGoBackAtom, canGoForwardAtom, goBack, goForward, pushHistory } from '../state/navigationHistory';
import { getShortcut, matchesShortcutDef } from '../shared/lib/shortcuts';
import { KbdTooltip } from '../shared/ui/KbdTooltip';
import { WindowControls } from '../shared/ui/WindowControls';
import { WorkspaceSwitcher } from '../shared/ui/WorkspaceSwitcher';
import { listen } from '../platform/ipc';
import styles from './AppShell.module.css';

let settleStarted = false;
function ensureSettle() {
  if (!settleStarted) {
    settleStarted = true;
    startSidebarSettle();
    startGridSettle();
    startInspectorSync();
  }
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

/** Scope title — frozen during grid transitions, live otherwise. */
function ScopeTitle() {
  const gridActive = useAtomValue(gridActiveAtom);
  const frozenLabel = useAtomValue(displayedScopeLabelAtom);
  const liveLabel = useAtomValue(gridScopeLabelAtom);
  const label = gridActive ? (frozenLabel || liveLabel) : liveLabel;
  if (!label) return null;
  return <span className={styles.scopeTitle}>{label}</span>;
}

export function AppShell() {
  const sidebarCollapsed = useAtomValue(sidebarCollapsedAtom);
  const inspectorCollapsed = useAtomValue(inspectorCollapsedAtom);
  const gridActive = useAtomValue(gridActiveAtom);
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const subscriptionsWorkspaceTab = useAtomValue(subscriptionsWorkspaceTabAtom);
  const viewerSession = useAtomValue(viewerSessionAtom);
  const canBack = useAtomValue(canGoBackAtom);
  const canForward = useAtomValue(canGoForwardAtom);
  const toggleSidebar = useSetAtom(toggleSidebarAtom);
  const toggleInspector = useSetAtom(toggleInspectorAtom);
  const setActiveNodeId = useSetAtom(activeNodeIdAtom);
  const setSubscriptionsWorkspaceTab = useSetAtom(setSubscriptionsWorkspaceTabAtom);

  const toggleBothPanels = () => { toggleSidebar(); toggleInspector(); };
  const isSubscriptionsWorkspace = activeNodeId === 'system:subscriptions';

  useEffect(() => { ensureSettle(); }, []);

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
              {isSubscriptionsWorkspace ? (
                <WorkspaceSwitcher
                  value={subscriptionsWorkspaceTab}
                  onChange={setSubscriptionsWorkspaceTab}
                  options={[
                    { value: 'subscriptions', label: 'Subscriptions', icon: <IconAntennaBars5 size={14} stroke={1.5} /> },
                    { value: 'auth', label: 'Auth', icon: <IconLock size={14} stroke={1.5} /> },
                  ]}
                />
              ) : gridActive ? (
                <GridToolbar />
              ) : null}
              {!showInspector && <WindowControls />}
            </>
          )}
        </div>
        {showInspector && (
          <div className={styles.titlebarInspector}>
            <InspectorTitlebarActions />
            <WindowControls />
          </div>
        )}
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
        {showInspector && (
          <div className={styles.inspector}>
            <Inspector />
          </div>
        )}
      </div>
    </div>
  );
}

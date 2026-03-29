/**
 * App shell — titlebar + sidebar + main content area.
 *
 * Titlebar is a drag region. Sidebar toggle, inspector toggle, and settings
 * buttons are right-aligned in the titlebar-left section.
 */

import { useEffect } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconLayoutSidebar, IconSettings } from '@tabler/icons-react';
import { Sidebar } from '../features/sidebar/Sidebar';
import { GridScreen } from '../features/grid/GridScreen';
import { GridToolbar } from '../features/grid/GridToolbar';
import { Inspector } from '../features/inspector/Inspector';
import {
  sidebarCollapsedAtom, toggleSidebarAtom,
  inspectorCollapsedAtom, toggleInspectorAtom,
} from '../state/navigation';
import { gridActiveAtom } from '../state/grid';
import { startSidebarSettle } from '../runtime/sidebarSettle';
import { startGridSettle } from '../runtime/gridSettle';
import { startInspectorSync } from '../controllers/inspectorController';
import { zoomController } from '../controllers/zoomController';
import { goBack, goForward } from '../state/navigationHistory';
import { getShortcut, matchesShortcutDef } from '../shared/lib/shortcuts';
import { KbdTooltip } from '../shared/ui/KbdTooltip';
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

function openSettings() {
  (window as any).picto?.api?.invoke('open_settings_window')?.catch(() => {});
}

export function AppShell() {
  const sidebarCollapsed = useAtomValue(sidebarCollapsedAtom);
  const inspectorCollapsed = useAtomValue(inspectorCollapsedAtom);
  const gridActive = useAtomValue(gridActiveAtom);
  const toggleSidebar = useSetAtom(toggleSidebarAtom);
  const toggleInspector = useSetAtom(toggleInspectorAtom);

  const toggleBothPanels = () => { toggleSidebar(); toggleInspector(); };

  useEffect(() => { ensureSettle(); }, []);

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

  const showInspector = gridActive && !inspectorCollapsed;

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
          {gridActive && <GridToolbar />}
        </div>
        {showInspector && <div className={styles.titlebarInspector} />}
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

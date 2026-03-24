/**
 * App shell — titlebar + sidebar + main content area.
 *
 * Titlebar is a drag region split into sidebar-colored left and main-colored right.
 * Sidebar toggle button lives in the titlebar left section.
 */

import { useEffect } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconLayoutSidebar } from '@tabler/icons-react';
import { Sidebar } from '../features/sidebar/Sidebar';
import { GridScreen } from '../features/grid/GridScreen';
import { GridToolbar } from '../features/grid/GridToolbar';
import { sidebarCollapsedAtom, toggleSidebarAtom } from '../state/navigation';
import { gridActiveAtom } from '../state/grid';
import { startSidebarSettle } from '../runtime/sidebarSettle';
import { startGridSettle } from '../runtime/gridSettle';
import { zoomController } from '../controllers/zoomController';
import styles from './AppShell.module.css';

let settleStarted = false;
function ensureSettle() {
  if (!settleStarted) {
    settleStarted = true;
    startSidebarSettle();
    startGridSettle();
  }
}

export function AppShell() {
  const sidebarCollapsed = useAtomValue(sidebarCollapsedAtom);
  const gridActive = useAtomValue(gridActiveAtom);
  const toggleSidebar = useSetAtom(toggleSidebarAtom);

  useEffect(() => { ensureSettle(); }, []);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (!e.metaKey && !e.ctrlKey) return;
      if (e.key === '\\') { e.preventDefault(); toggleSidebar(); }
      else if (e.key === '=' || e.key === '+') { e.preventDefault(); zoomController.zoomIn(); }
      else if (e.key === '-') { e.preventDefault(); zoomController.zoomOut(); }
      else if (e.key === '0') { e.preventDefault(); zoomController.resetZoom(); }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [toggleSidebar]);

  return (
    <div className={styles.shell}>
      {/* Titlebar — drag region, sidebar-colored left + main-colored right */}
      <div className={styles.titlebar}>
        <div className={sidebarCollapsed ? styles.titlebarLeftCollapsed : styles.titlebarLeft}>
          <div className={styles.titlebarActions}>
            <button className={styles.toggleBtn} onClick={() => toggleSidebar()} title={sidebarCollapsed ? 'Show sidebar (⌘\\)' : 'Hide sidebar (⌘\\)'}>
              <IconLayoutSidebar size={18} stroke={1.5} />
            </button>
          </div>
        </div>
        <div className={styles.titlebarRight}>
          {gridActive && <GridToolbar />}
        </div>
      </div>

      {/* Body — sidebar + main */}
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
    </div>
  );
}

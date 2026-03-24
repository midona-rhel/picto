/**
 * App shell — root layout with sidebar and main content area.
 *
 * The sidebar is the first rebuilt live slice (PBI-591).
 * The main content area is a placeholder until PBI-592 (grid rebuild).
 */

import { useEffect } from 'react';
import { useAtomValue } from 'jotai';
import { Sidebar } from '../features/sidebar/Sidebar';
import { activeNodeIdAtom } from '../state/navigation';
import { startSidebarSettle } from '../runtime/sidebarSettle';
import styles from './AppShell.module.css';

// Start sidebar runtime settle once at module load
let settleStarted = false;
function ensureSidebarSettle() {
  if (!settleStarted) {
    settleStarted = true;
    startSidebarSettle();
  }
}

export function AppShell() {
  const activeNodeId = useAtomValue(activeNodeIdAtom);

  useEffect(() => {
    ensureSidebarSettle();
  }, []);

  return (
    <div className={styles.shell}>
      <div className={styles.sidebar}>
        <Sidebar />
      </div>
      <div className={styles.main}>
        {/* Grid placeholder — PBI-592 will replace this */}
        <span>{activeNodeId}</span>
      </div>
    </div>
  );
}

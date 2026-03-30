/**
 * WindowControls — minimize, maximize, close buttons for the titlebar.
 * Matches legacy v0.5.0-alpha styling. Always visible (useful for debugging
 * on Mac too; on production Mac builds, native traffic lights are used instead).
 */

import { IconMinus, IconSquare, IconX } from '@tabler/icons-react';
import styles from './WindowControls.module.css';

function callWindow(method: string) {
  (window as any).picto?.window?.call(method)?.catch?.(() => {});
}

export function WindowControls() {
  return (
    <div className={styles.controls}>
      <button className={styles.btn} onClick={() => callWindow('minimize')} title="Minimize">
        <IconMinus size={16} stroke={1} />
      </button>
      <button className={styles.btn} onClick={() => callWindow('toggleMaximize')} title="Maximize">
        <IconSquare size={12} stroke={1.5} />
      </button>
      <button className={`${styles.btn} ${styles.closeBtn}`} onClick={() => callWindow('close')} title="Close">
        <IconX size={16} stroke={1} />
      </button>
    </div>
  );
}

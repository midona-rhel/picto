/**
 * WindowControls — minimize, maximize, close buttons for the titlebar.
 * Matches legacy v0.5.0-alpha styling. Always visible (useful for debugging
 * on Mac too; on production Mac builds, native traffic lights are used instead).
 */

import { IconMinus, IconSquare, IconX } from '@tabler/icons-react';
import { KbdTooltip } from '../KbdTooltip';
import styles from './WindowControls.module.css';

function callWindow(method: string) {
  (window as any).picto?.window?.call(method)?.catch?.(() => {});
}

export function WindowControls() {
  return (
    <div className={styles.controls}>
      <KbdTooltip label="Minimize"><button className={styles.btn} onClick={() => callWindow('minimize')} aria-label="Minimize">
        <IconMinus size={16} stroke={1} />
      </button></KbdTooltip>
      <KbdTooltip label="Maximize"><button className={styles.btn} onClick={() => callWindow('toggleMaximize')} aria-label="Maximize">
        <IconSquare size={12} stroke={1.5} />
      </button></KbdTooltip>
      <KbdTooltip label="Close"><button className={`${styles.btn} ${styles.closeBtn}`} onClick={() => callWindow('close')} aria-label="Close">
        <IconX size={16} stroke={1} />
      </button></KbdTooltip>
    </div>
  );
}

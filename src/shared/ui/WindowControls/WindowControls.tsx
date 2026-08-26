/**
 * WindowControls — minimize, maximize, close buttons for the titlebar.
 * Windows uses Picto's custom titlebar controls. Other platforms keep their
 * native window chrome, so rendering a second control set is both redundant
 * and displaces inspector actions such as Pin.
 */

import { IconMinus, IconSquare, IconX } from '@tabler/icons-react';
import { KbdTooltip } from '../KbdTooltip';
import styles from './WindowControls.module.css';

function callWindow(method: string) {
  (window as any).picto?.window?.call(method)?.catch?.(() => {});
}

export function WindowControls({
  platform = navigator.platform,
}: {
  platform?: string;
} = {}) {
  if (!/^Win/i.test(platform)) return null;

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

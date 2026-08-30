/**
 * WindowControls — minimize, maximize, close buttons for the titlebar.
 * Windows uses Picto's custom titlebar controls. Other platforms keep their
 * native window chrome, so rendering a second control set is both redundant
 * and displaces inspector actions such as Pin.
 */

import { IconMinus, IconSquare } from '@tabler/icons-react';
import { KbdTooltip } from '../KbdTooltip';
import { WindowCloseButton } from './WindowCloseButton';
import styles from './WindowControls.module.css';

function callWindow(method: string) {
  (window as any).picto?.api?.window?.call(method)?.catch?.(() => {});
}

export function WindowControls({
  platform = navigator.platform,
}: {
  platform?: string;
} = {}) {
  if (!/^(Win|Linux)/i.test(platform)) return null;

  return (
    <div className={styles.controls}>
      <KbdTooltip label="Minimize"><button type="button" className={styles.btn} onClick={() => callWindow('minimize')} aria-label="Minimize">
        <IconMinus size={16} stroke={1} />
      </button></KbdTooltip>
      <KbdTooltip label="Maximize"><button type="button" className={styles.btn} onClick={() => callWindow('toggleMaximize')} aria-label="Maximize">
        <IconSquare size={12} stroke={1.5} />
      </button></KbdTooltip>
      <KbdTooltip label="Close"><WindowCloseButton /></KbdTooltip>
    </div>
  );
}

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
import { t } from '../../../i18n';

type WindowControlMethod = 'minimize' | 'toggleMaximize';

function callWindow(method: WindowControlMethod) {
  (window as any).picto?.windowControls?.[method]?.();
}

export function WindowControls({
  platform = navigator.platform,
}: {
  platform?: string;
} = {}) {
  if (!/^(Win|Linux)/i.test(platform)) return null;

  return (
    <div className={styles.controls}>
      <KbdTooltip label={t("Minimize")}><button type="button" className={styles.btn} onClick={() => callWindow('minimize')} aria-label={t("Minimize")}>
        <IconMinus size={16} stroke={1} />
      </button></KbdTooltip>
      <KbdTooltip label={t("Maximize")}><button type="button" className={styles.btn} onClick={() => callWindow('toggleMaximize')} aria-label={t("Maximize")}>
        <IconSquare size={12} stroke={1.5} />
      </button></KbdTooltip>
      <KbdTooltip label={t("Close")}><WindowCloseButton destructive /></KbdTooltip>
    </div>
  );
}

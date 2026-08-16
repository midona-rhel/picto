import { IconMenu2 } from '@tabler/icons-react';
import { KbdTooltip } from '../KbdTooltip';
import styles from './ApplicationMenuButton.module.css';

export function usesInWindowApplicationMenu(platform = navigator.platform) {
  return !/^mac/i.test(platform);
}

/**
 * Opens the one native application menu built in the Electron main process.
 * macOS keeps that menu in the system menu bar, so it does not need a duplicate
 * in-window trigger.
 */
export function ApplicationMenuButton() {
  if (!usesInWindowApplicationMenu()) return null;

  return (
    <KbdTooltip label="Application menu" position="bottom">
      <button
        type="button"
        className={styles.button}
        aria-label="Application menu"
        aria-haspopup="menu"
        onClick={() => { void (window as any).picto?.popupMenu?.(); }}
      >
        <IconMenu2 size={18} stroke={1.6} aria-hidden="true" />
      </button>
    </KbdTooltip>
  );
}

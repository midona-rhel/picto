import { useCallback } from 'react';
import { IconMinus, IconSquare, IconX } from '@tabler/icons-react';
import { getCurrentWindow } from '#desktop/api';
import { runBestEffort } from '../../../shared/lib/asyncOps';
import styles from './WindowControls.module.css';

export function WindowControls() {
  const win = getCurrentWindow();

  const handleMinimize = useCallback(() => {
    runBestEffort('window.minimize', win.minimize());
  }, [win]);
  const handleMaximize = useCallback(() => {
    runBestEffort('window.toggleMaximize', win.toggleMaximize());
  }, [win]);
  const handleClose = useCallback(() => {
    runBestEffort('window.close', win.close());
  }, [win]);

  return (
    <div className={styles.controls}>
      <button className={styles.btn} onClick={handleMinimize} aria-label="Minimize">
        <IconMinus size={14} />
      </button>
      <button className={styles.btn} onClick={handleMaximize} aria-label="Maximize">
        <IconSquare size={12} />
      </button>
      <button className={`${styles.btn} ${styles.closeBtn}`} onClick={handleClose} aria-label="Close">
        <IconX size={14} />
      </button>
    </div>
  );
}

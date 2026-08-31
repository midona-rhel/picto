import { IconArrowBarToRight } from '@tabler/icons-react';
import { t } from '../../../i18n';
import styles from './OverlayShell.module.css';

/** Compact visual hint for the Tab key used by selection portals. */
export function TabKeyHint() {
  return (
    <span className={`${styles.kbd} ${styles.tabKeyIcon}`} role="img" aria-label={t('Tab')}>
      <IconArrowBarToRight size={13} stroke={1.8} aria-hidden="true" />
    </span>
  );
}

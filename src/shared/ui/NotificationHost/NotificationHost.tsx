import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import {
  IconAlertCircleFilled,
  IconCircleCheckFilled,
  IconCircleXFilled,
  IconInfoCircleFilled,
  IconX,
} from '@tabler/icons-react';
import {
  dismissNotification,
  getCurrentNotification,
  subscribeToNotifications,
  type AppNotification,
  type NotificationTone,
} from '../../lib/notifications';
import styles from './NotificationHost.module.css';

const EXIT_DURATION_MS = 400;

const toneIcons = {
  error: IconCircleXFilled,
  warning: IconAlertCircleFilled,
  info: IconInfoCircleFilled,
  success: IconCircleCheckFilled,
} satisfies Record<NotificationTone, typeof IconAlertCircleFilled>;

export function NotificationHost() {
  const notification = useSyncExternalStore(
    subscribeToNotifications,
    getCurrentNotification,
    getCurrentNotification,
  );
  const [displayed, setDisplayed] = useState<AppNotification | null>(notification);
  const [visible, setVisible] = useState(notification != null);
  const exitTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (exitTimer.current) clearTimeout(exitTimer.current);
    if (notification) {
      setDisplayed(notification);
      setVisible(true);
      return;
    }
    setVisible(false);
    exitTimer.current = setTimeout(() => setDisplayed(null), EXIT_DURATION_MS);
    return () => {
      if (exitTimer.current) clearTimeout(exitTimer.current);
    };
  }, [notification]);

  useEffect(() => {
    if (!notification) return;
    const timer = setTimeout(
      () => dismissNotification(notification.id),
      notification.duration,
    );
    return () => clearTimeout(timer);
  }, [notification]);

  if (!displayed) return null;
  const StatusIcon = toneIcons[displayed.tone];
  const urgent = displayed.tone === 'error' || displayed.tone === 'warning';

  return (
    <div className={styles.layer} aria-live={urgent ? 'assertive' : 'polite'}>
      <div
        className={`${styles.notification} ${styles[displayed.tone]} ${visible ? styles.visible : styles.hidden}`}
        role={urgent ? 'alert' : 'status'}
      >
        <StatusIcon className={styles.statusIcon} size={24} aria-hidden="true" />
        <div className={styles.message}>
          <span className={styles.title}>{displayed.title}</span>
          {displayed.message ? <span className={styles.detail}> — {displayed.message}</span> : null}
          {displayed.action ? (
            <button
              type="button"
              className={styles.action}
              onClick={() => {
                displayed.action?.onClick();
                dismissNotification(displayed.id);
              }}
            >
              {displayed.action.label}
            </button>
          ) : null}
        </div>
        <button
          type="button"
          className={styles.close}
          aria-label="Dismiss notification"
          onClick={() => dismissNotification(displayed.id)}
        >
          <IconX size={14} stroke={1.75} aria-hidden="true" />
        </button>
      </div>
    </div>
  );
}

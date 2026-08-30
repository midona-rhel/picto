import { IconX } from '@tabler/icons-react';
import { forwardRef } from 'react';
import styles from './WindowControls.module.css';

function closeCurrentWindow() {
  const close = (window as any).picto?.windowControls?.close;
  if (typeof close === 'function') close();
  else window.close();
}

export const WindowCloseButton = forwardRef<HTMLButtonElement, {
  onClick?: () => void;
  ariaLabel?: string;
  disabled?: boolean;
  destructive?: boolean;
}>(function WindowCloseButton({
  onClick = closeCurrentWindow,
  ariaLabel = 'Close',
  disabled = false,
  destructive = false,
}, ref) {
  return (
    <button
      ref={ref}
      className={`${styles.btn} ${destructive ? styles.destructiveClose : ''}`}
      onClick={onClick}
      aria-label={ariaLabel}
      data-destructive={destructive || undefined}
      type="button"
      disabled={disabled}
    >
      <IconX size={16} stroke={1} />
    </button>
  );
});

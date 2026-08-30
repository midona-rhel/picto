import { IconX } from '@tabler/icons-react';
import { forwardRef } from 'react';
import styles from './WindowControls.module.css';

function closeCurrentWindow() {
  const request = (window as any).picto?.api?.window?.call('close');
  if (request?.catch) request.catch(() => window.close());
  else window.close();
}

export const WindowCloseButton = forwardRef<HTMLButtonElement, {
  onClick?: () => void;
  ariaLabel?: string;
  disabled?: boolean;
}>(function WindowCloseButton({
  onClick = closeCurrentWindow,
  ariaLabel = 'Close',
  disabled = false,
}, ref) {
  return (
    <button ref={ref} className={`${styles.btn} ${styles.closeBtn}`} onClick={onClick} aria-label={ariaLabel} title={ariaLabel} type="button" disabled={disabled}>
      <IconX size={16} stroke={1} />
    </button>
  );
});

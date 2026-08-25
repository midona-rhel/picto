import type { ReactNode } from 'react';
import styles from '../SubscriptionsScreen.module.css';

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger';

export function ActionButton({
  variant = 'secondary',
  compact = false,
  disabled = false,
  onClick,
  children,
}: {
  variant?: Variant;
  compact?: boolean;
  disabled?: boolean;
  onClick?: () => void;
  children: ReactNode;
}) {
  const variantClass = variant === 'primary'
    ? styles.button
    : variant === 'danger'
      ? styles.buttonDanger
      : variant === 'ghost'
        ? styles.buttonGhost
        : styles.buttonSecondary;

  return (
    <button
      className={`${variantClass} ${compact ? styles.buttonCompact : ''}`.trim()}
      data-modal-primary={variant === 'primary' ? 'true' : undefined}
      disabled={disabled}
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  );
}

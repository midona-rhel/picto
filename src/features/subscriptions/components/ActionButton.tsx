import type { ReactNode } from 'react';
import styles from '../SubscriptionsScreen.module.css';

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger';

export function ActionButton({
  variant = 'secondary',
  compact = false,
  disabled = false,
  pending = false,
  onClick,
  children,
}: {
  variant?: Variant;
  compact?: boolean;
  disabled?: boolean;
  /** Blocks repeat activation without visually dimming an in-flight action. */
  pending?: boolean;
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
      className={`${variantClass} ${compact ? styles.buttonCompact : ''} ${pending ? styles.buttonPending : ''}`.trim()}
      data-modal-primary={variant === 'primary' ? 'true' : undefined}
      disabled={disabled || pending}
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  );
}

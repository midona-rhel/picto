/**
 * ToggleSwitch — reusable iOS-style toggle.
 * 32×20px track, 16px sliding knob. Matches legacy inline styles 1:1.
 */

import styles from './ToggleSwitch.module.css';

interface Props {
  on: boolean;
  onChange: () => void;
  disabled?: boolean;
  ariaLabel?: string;
}

export function ToggleSwitch({ on, onChange, disabled = false, ariaLabel }: Props) {
  return (
    <label
      className={`${styles.toggle} ${disabled ? styles.disabled : ''}`}
      role="switch"
      aria-label={ariaLabel}
      aria-checked={on}
      aria-disabled={disabled}
      tabIndex={disabled ? -1 : 0}
      onClick={(event) => {
        event.stopPropagation();
        if (disabled) return;
        onChange();
      }}
      onKeyDown={(event) => {
        if (disabled) return;
        if (event.key !== 'Enter' && event.key !== ' ') return;
        event.preventDefault();
        event.stopPropagation();
        onChange();
      }}
    >
      <span className={`${styles.track} ${on ? styles.trackOn : ''}`} />
      <span className={`${styles.knob} ${on ? styles.knobOn : ''}`} />
    </label>
  );
}

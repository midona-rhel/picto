/**
 * ToggleSwitch — reusable iOS-style toggle.
 * 32×20px track, 16px sliding knob. Matches legacy inline styles 1:1.
 */

import styles from './ToggleSwitch.module.css';

interface Props {
  on: boolean;
  onChange: () => void;
}

export function ToggleSwitch({ on, onChange }: Props) {
  return (
    <label className={styles.toggle} onClick={(e) => { e.stopPropagation(); onChange(); }}>
      <span className={`${styles.track} ${on ? styles.trackOn : ''}`} />
      <span className={`${styles.knob} ${on ? styles.knobOn : ''}`} />
    </label>
  );
}

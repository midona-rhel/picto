/**
 * VolumeHUD — centered overlay that flashes volume percentage on change.
 * Similar to macOS volume OSD.
 */

import { useEffect, useRef, useState } from 'react';
import { IconVolume, IconVolume2, IconVolumeOff } from '@tabler/icons-react';
import { VOLUME_HUD_DURATION } from './videoConstants';
import styles from './VideoPlayer.module.css';

interface VolumeHUDProps {
  volume: number;
  muted: boolean;
  /** Increment to trigger a flash (e.g. Date.now() on each scroll-volume event). */
  trigger: number;
}

export function VolumeHUD({ volume, muted, trigger }: VolumeHUDProps) {
  const [visible, setVisible] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout>>();
  const initialRenderRef = useRef(true);

  const effectiveVolume = muted ? 0 : volume;
  const VolumeIcon = muted || volume === 0
    ? IconVolumeOff
    : volume < 0.5
      ? IconVolume2
      : IconVolume;

  useEffect(() => {
    // Don't show on initial render
    if (initialRenderRef.current) {
      initialRenderRef.current = false;
      return;
    }
    if (trigger === 0) return;

    setVisible(true);
    clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => setVisible(false), VOLUME_HUD_DURATION);

    return () => clearTimeout(timerRef.current);
  }, [trigger]);

  if (!visible) return null;

  return (
    <div className={styles.volumeHud}>
      <VolumeIcon size={28} />
      <span className={styles.volumeHudPercent}>{Math.round(effectiveVolume * 100)}%</span>
    </div>
  );
}

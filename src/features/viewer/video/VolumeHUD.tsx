import { useEffect, useRef, useState } from 'react';
import { IconVolume, IconVolume2, IconVolumeOff } from '@tabler/icons-react';
import { VOLUME_HUD_DURATION } from './videoConstants';
import styles from './VideoPlayer.module.css';

interface Props { volume: number; muted: boolean; trigger: number; }

export function VolumeHUD({ volume, muted, trigger }: Props) {
  const [visible, setVisible] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout>>();
  const initRef = useRef(true);
  const eff = muted ? 0 : volume;
  const Icon = muted || volume === 0 ? IconVolumeOff : volume < 0.5 ? IconVolume2 : IconVolume;

  useEffect(() => {
    if (initRef.current) { initRef.current = false; return; }
    if (trigger === 0) return;
    setVisible(true);
    clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => setVisible(false), VOLUME_HUD_DURATION);
    return () => clearTimeout(timerRef.current);
  }, [trigger]);

  if (!visible) return null;
  return (
    <div className={styles.volumeHud}>
      <Icon size={28} />
      <span className={styles.volumeHudPercent}>{Math.round(eff * 100)}%</span>
    </div>
  );
}

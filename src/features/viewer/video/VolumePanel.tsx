import { useCallback, useEffect, useRef, useState } from 'react';
import { IconVolume, IconVolume2, IconVolumeOff } from '@tabler/icons-react';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import styles from './VideoPlayer.module.css';

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

interface Props {
  volume: number;
  muted: boolean;
  onVolumeChange: (v: number) => void;
  onMuteToggle: () => void;
}

export function VolumePanel({ volume, muted, onVolumeChange, onMuteToggle }: Props) {
  const [show, setShow] = useState(false);
  const [dragging, setDragging] = useState(false);
  const trackRef = useRef<HTMLDivElement>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout>>();
  const eff = muted ? 0 : volume;
  const Icon = muted || volume === 0 ? IconVolumeOff : volume < 0.5 ? IconVolume2 : IconVolume;

  const enter = useCallback(() => { clearTimeout(timerRef.current); setShow(true); }, []);
  const leave = useCallback(() => { if (!dragging) timerRef.current = setTimeout(() => setShow(false), 300); }, [dragging]);
  useEffect(() => () => clearTimeout(timerRef.current), []);

  const getVol = useCallback((e: MouseEvent | React.MouseEvent) => {
    const t = trackRef.current; if (!t) return 0;
    const r = t.getBoundingClientRect();
    return clamp(1 - (e.clientY - r.top) / r.height, 0, 1);
  }, []);

  const handleDown = useCallback((e: React.MouseEvent) => { e.preventDefault(); e.stopPropagation(); setDragging(true); onVolumeChange(getVol(e)); }, [getVol, onVolumeChange]);

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => onVolumeChange(getVol(e));
    const onUp = () => setDragging(false);
    window.addEventListener('mousemove', onMove); window.addEventListener('mouseup', onUp);
    return () => { window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp); };
  }, [dragging, getVol, onVolumeChange]);

  return (
    <div className={styles.volumePanel} onMouseEnter={enter} onMouseLeave={leave}>
      <KbdTooltip label={muted ? 'Unmute' : 'Mute'} shortcut="M">
        <button className={styles.icBtn} aria-label={muted ? 'Unmute' : 'Mute'} onClick={(e) => { e.stopPropagation(); onMuteToggle(); }}>
          <Icon size={16} />
        </button>
      </KbdTooltip>
      {show && (
        <div className={styles.volumePopup}>
          <div className={styles.volumePercent}>{Math.round(eff * 100)}%</div>
          <div ref={trackRef} className={styles.volumeTrack} onMouseDown={handleDown}>
            <div className={styles.volumeFill} style={{ height: `${eff * 100}%` }} />
          </div>
        </div>
      )}
    </div>
  );
}

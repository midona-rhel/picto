import { useCallback, useEffect, useRef, useState } from 'react';
import { formatTime } from './videoTimeFormat';
import styles from './VideoPlayer.module.css';

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

interface Props {
  currentTime: number;
  duration: number;
  buffered: TimeRanges | null;
  onSeek: (time: number) => void;
  onSeekStart?: () => void;
  onSeekEnd?: () => void;
  waveformSrc?: string;
}

export function ProgressBar({ currentTime, duration, buffered, onSeek, onSeekStart, onSeekEnd, waveformSrc }: Props) {
  const trackRef = useRef<HTMLDivElement>(null);
  const [hovered, setHovered] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [hoverFrac, setHoverFrac] = useState(0);

  const getFrac = useCallback((e: MouseEvent | React.MouseEvent) => {
    const t = trackRef.current;
    if (!t) return 0;
    const r = t.getBoundingClientRect();
    return clamp((e.clientX - r.left) / r.width, 0, 1);
  }, []);

  const handleDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault(); e.stopPropagation();
    setDragging(true); onSeekStart?.();
    onSeek(getFrac(e) * duration);
  }, [duration, getFrac, onSeek, onSeekStart]);

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => { const f = getFrac(e); setHoverFrac(f); onSeek(f * duration); };
    const onUp = () => { setDragging(false); onSeekEnd?.(); };
    window.addEventListener('mousemove', onMove); window.addEventListener('mouseup', onUp);
    return () => { window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp); };
  }, [dragging, duration, getFrac, onSeek, onSeekEnd]);

  const progress = duration > 0 ? clamp(currentTime / duration, 0, 1) : 0;
  let bufferedFrac = 0;
  if (buffered && buffered.length > 0 && duration > 0) bufferedFrac = clamp(buffered.end(buffered.length - 1) / duration, 0, 1);
  const expanded = hovered || dragging;

  return (
    <div ref={trackRef} className={`${styles.progressBar} ${waveformSrc ? styles.waveformProgressBar : ''}`}
      onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)}
      onMouseMove={(e) => setHoverFrac(getFrac(e))} onMouseDown={handleDown}>
      {waveformSrc ? (
        <>
          <div
            className={styles.waveformBase}
            data-waveform-layer="unplayed"
            style={{ WebkitMaskImage: `url("${waveformSrc}")`, maskImage: `url("${waveformSrc}")` }}
          />
          <div className={styles.waveformPlayed} style={{ clipPath: `inset(0 ${(1 - progress) * 100}% 0 0)` }}>
            <div
              className={styles.waveformImage}
              data-waveform-layer="played"
              style={{ WebkitMaskImage: `url("${waveformSrc}")`, maskImage: `url("${waveformSrc}")` }}
            />
          </div>
          <div className={styles.waveformCursor} style={{ left: `${progress * 100}%` }} />
        </>
      ) : (
        <div className={`${styles.progressTrack} ${expanded ? styles.progressTrackExpanded : ''}`}>
          <div className={styles.progressBuffered} style={{ width: `${bufferedFrac * 100}%` }} />
          <div className={styles.progressFill} style={{ width: `${progress * 100}%` }} />
        </div>
      )}
      {(hovered || dragging) && duration > 0 && (
        <div className={styles.progressHoverTime} style={{ left: `${hoverFrac * 100}%` }}>
          {formatTime(hoverFrac * duration)}
        </div>
      )}
    </div>
  );
}

/**
 * VolumePanel — mute button + vertical slider popup on hover.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { IconVolume, IconVolume2, IconVolumeOff } from '@tabler/icons-react';
import styles from './VideoPlayer.module.css';

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

interface VolumePanelProps {
  volume: number;
  muted: boolean;
  onVolumeChange: (v: number) => void;
  onMuteToggle: () => void;
}

export function VolumePanel({ volume, muted, onVolumeChange, onMuteToggle }: VolumePanelProps) {
  const [showSlider, setShowSlider] = useState(false);
  const [dragging, setDragging] = useState(false);
  const trackRef = useRef<HTMLDivElement>(null);
  const hideTimerRef = useRef<ReturnType<typeof setTimeout>>();

  const effectiveVolume = muted ? 0 : volume;

  const VolumeIcon = muted || volume === 0
    ? IconVolumeOff
    : volume < 0.5
      ? IconVolume2
      : IconVolume;

  const handleMouseEnter = useCallback(() => {
    clearTimeout(hideTimerRef.current);
    setShowSlider(true);
  }, []);

  const handleMouseLeave = useCallback(() => {
    if (!dragging) {
      hideTimerRef.current = setTimeout(() => setShowSlider(false), 300);
    }
  }, [dragging]);

  useEffect(() => {
    return () => clearTimeout(hideTimerRef.current);
  }, []);

  const getVolumeFromEvent = useCallback((e: MouseEvent | React.MouseEvent) => {
    const track = trackRef.current;
    if (!track) return 0;
    const rect = track.getBoundingClientRect();
    // Vertical slider: bottom = 0, top = 1
    return clamp(1 - (e.clientY - rect.top) / rect.height, 0, 1);
  }, []);

  const handleSliderMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setDragging(true);
    onVolumeChange(getVolumeFromEvent(e));
  }, [getVolumeFromEvent, onVolumeChange]);

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => onVolumeChange(getVolumeFromEvent(e));
    const onUp = () => {
      setDragging(false);
      // Check if mouse is still over panel; if not, hide
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [dragging, getVolumeFromEvent, onVolumeChange]);

  return (
    <div
      className={styles.volumePanel}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      {/* Vertical slider popup */}
      {showSlider && (
        <div className={styles.volumePopup}>
          <div className={styles.volumePercent}>
            {Math.round(effectiveVolume * 100)}%
          </div>
          <div
            ref={trackRef}
            className={styles.volumeTrack}
            onMouseDown={handleSliderMouseDown}
          >
            <div
              className={styles.volumeFill}
              style={{ height: `${effectiveVolume * 100}%` }}
            />
          </div>
        </div>
      )}

      {/* Mute toggle button */}
      <button
        className={styles.icBtn}
        onClick={(e) => { e.stopPropagation(); onMuteToggle(); }}
        title={muted ? 'Unmute' : 'Mute'}
      >
        <VolumeIcon size={16} />
      </button>
    </div>
  );
}

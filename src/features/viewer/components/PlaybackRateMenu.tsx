/**
 * PlaybackRateMenu — dropdown for playback rate selection.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { PLAYBACK_RATES } from './videoConstants';
import styles from './VideoPlayer.module.css';

interface PlaybackRateMenuProps {
  rate: number;
  onRateChange: (rate: number) => void;
}

export function PlaybackRateMenu({ rate, onRateChange }: PlaybackRateMenuProps) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const toggleMenu = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    setOpen((prev) => !prev);
  }, []);

  const handleSelect = useCallback((r: number) => {
    onRateChange(r);
    setOpen(false);
  }, [onRateChange]);

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    const handleClick = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    // Use capture to catch clicks before stopPropagation
    document.addEventListener('mousedown', handleClick, true);
    return () => document.removeEventListener('mousedown', handleClick, true);
  }, [open]);

  const displayRate = rate === 1 ? '1x' : `${rate}x`;

  return (
    <div ref={containerRef} className={styles.rateMenuContainer}>
      <button
        className={`${styles.icBtn} ${styles.rateButton}`}
        onClick={toggleMenu}
        title="Playback speed"
      >
        {displayRate}
      </button>

      {open && (
        <div className={styles.rateMenu}>
          {PLAYBACK_RATES.map((r) => (
            <button
              key={r}
              className={`${styles.rateMenuItem} ${r === rate ? styles.rateMenuItemActive : ''}`}
              onClick={(e) => { e.stopPropagation(); handleSelect(r); }}
            >
              {r}x
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

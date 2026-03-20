/**
 * VideoScrubOverlay — portal overlay that replaces a grid thumbnail with
 * a seekable video when the user hovers over a video tile for >500ms.
 *
 * Moving the cursor left/right seeks to an absolute position in the video.
 * A thin progress bar at the bottom indicates the current position.
 *
 * The overlay uses pointer-events: none so clicks pass through to the grid
 * for selection/context menu. Mouse tracking uses a window-level listener.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { formatDuration } from '../../shared/lib/formatters';
import styles from './VideoScrubOverlay.module.css';

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

export interface VideoScrubRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

interface VideoScrubOverlayProps {
  tileRect: VideoScrubRect;
  src: string;
  duration: number;
  onDismiss: () => void;
}

export function VideoScrubOverlay({ tileRect, src, duration, onDismiss }: VideoScrubOverlayProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [fraction, setFraction] = useState(0);
  const rafRef = useRef<number>(0);
  const pendingFractionRef = useRef(0);
  const tileRectRef = useRef(tileRect);
  tileRectRef.current = tileRect;
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;
  const dismissedRef = useRef(false);

  const dismiss = useCallback(() => {
    if (dismissedRef.current) return;
    dismissedRef.current = true;
    onDismissRef.current();
  }, []);

  // Track mouse via window listener — overlay is pointer-events: none
  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const r = tileRectRef.current;
      // Dismiss if mouse leaves tile rect (with a small tolerance)
      const tolerance = 2;
      if (
        e.clientX < r.left - tolerance || e.clientX > r.left + r.width + tolerance ||
        e.clientY < r.top - tolerance || e.clientY > r.top + r.height + tolerance
      ) {
        dismiss();
        return;
      }

      const frac = clamp((e.clientX - r.left) / r.width, 0, 1);
      pendingFractionRef.current = frac;

      if (!rafRef.current) {
        rafRef.current = requestAnimationFrame(() => {
          rafRef.current = 0;
          const f = pendingFractionRef.current;
          setFraction(f);
          const video = videoRef.current;
          if (video && duration > 0) {
            video.currentTime = f * duration;
          }
        });
      }
    };

    // Dismiss on click or any keydown (Enter/Space/Escape etc.)
    const onMouseDown = () => dismiss();
    const onKeyDown = () => {
      dismiss();
    };

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mousedown', onMouseDown);
    window.addEventListener('keydown', onKeyDown, true); // capture phase for immediate dismiss
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mousedown', onMouseDown);
      window.removeEventListener('keydown', onKeyDown, true);
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
    };
  }, [duration, dismiss]);

  // Pad current time to same length as total duration (e.g. "0:03" for a "1:24" video)
  const totalTime = formatDuration(duration * 1000);
  const rawCurrentTime = formatDuration(fraction * duration * 1000);
  const currentTime = rawCurrentTime.length < totalTime.length
    ? rawCurrentTime.padStart(totalTime.length, '0')
    : rawCurrentTime;

  return createPortal(
    <div
      className={styles.overlay}
      style={{
        left: tileRect.left,
        top: tileRect.top,
        width: tileRect.width,
        height: tileRect.height,
      }}
    >
      <video
        ref={videoRef}
        src={src}
        muted
        preload="auto"
        className={styles.video}
      />
      {/* Time badge — shows current / total */}
      <span className={styles.timeBadge}>
        {currentTime}
      </span>
      {/* Progress bar */}
      <div className={styles.progressTrack}>
        <div className={styles.progressFill} style={{ width: `${fraction * 100}%` }} />
      </div>
    </div>,
    document.body,
  );
}

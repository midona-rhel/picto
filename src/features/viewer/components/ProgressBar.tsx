/**
 * ProgressBar — video seek bar with buffered range, hover time tooltip, and frame preview.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { formatTime } from './videoTimeFormat';
import styles from './VideoPlayer.module.css';

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

interface ProgressBarProps {
  currentTime: number;
  duration: number;
  buffered: TimeRanges | null;
  onSeek: (time: number) => void;
  onSeekStart?: () => void;
  onSeekEnd?: () => void;
  /** Video source URL — when set, enables frame preview on hover/drag */
  src?: string;
}

export function ProgressBar({ currentTime, duration, buffered, onSeek, onSeekStart, onSeekEnd, src }: ProgressBarProps) {
  const trackRef = useRef<HTMLDivElement>(null);
  const [hovered, setHovered] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [hoverFraction, setHoverFraction] = useState(0);

  // Preview video + canvas for scrub preview
  const previewVideoRef = useRef<HTMLVideoElement>(null);
  const previewCanvasRef = useRef<HTMLCanvasElement>(null);
  const [previewReady, setPreviewReady] = useState(false);
  const lastSeekTimeRef = useRef(-1);

  const getFractionFromEvent = useCallback((e: MouseEvent | React.MouseEvent) => {
    const track = trackRef.current;
    if (!track) return 0;
    const rect = track.getBoundingClientRect();
    return clamp((e.clientX - rect.left) / rect.width, 0, 1);
  }, []);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setDragging(true);
    onSeekStart?.();
    const frac = getFractionFromEvent(e);
    onSeek(frac * duration);
  }, [duration, getFractionFromEvent, onSeek, onSeekStart]);

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => {
      const frac = getFractionFromEvent(e);
      setHoverFraction(frac);
      onSeek(frac * duration);
    };
    const onUp = () => {
      setDragging(false);
      onSeekEnd?.();
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [dragging, duration, getFractionFromEvent, onSeek, onSeekEnd]);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    setHoverFraction(getFractionFromEvent(e));
  }, [getFractionFromEvent]);

  // Seek preview video when hover fraction changes (throttled by 0.05s delta)
  useEffect(() => {
    if (!(hovered || dragging) || !previewReady || duration <= 0) return;
    const targetTime = hoverFraction * duration;
    if (Math.abs(targetTime - lastSeekTimeRef.current) < 0.05) return;
    lastSeekTimeRef.current = targetTime;
    const video = previewVideoRef.current;
    if (video) video.currentTime = targetTime;
  }, [hoverFraction, hovered, dragging, previewReady, duration]);

  // Draw frame to canvas when preview video seeks
  // Re-register when previewReady changes (refs become available after mount)
  useEffect(() => {
    const video = previewVideoRef.current;
    const canvas = previewCanvasRef.current;
    if (!video || !canvas) return;

    const onSeeked = () => {
      const ctx = canvas.getContext('2d');
      if (!ctx) return;
      const vw = video.videoWidth;
      const vh = video.videoHeight;
      if (vw === 0 || vh === 0) return;
      const canvasW = 160;
      const canvasH = Math.round((vh / vw) * canvasW);
      canvas.width = canvasW;
      canvas.height = canvasH;
      ctx.drawImage(video, 0, 0, canvasW, canvasH);
    };

    video.addEventListener('seeked', onSeeked);
    return () => video.removeEventListener('seeked', onSeeked);
  }, [previewReady, src]);

  // Reset preview ready state when src changes
  useEffect(() => {
    setPreviewReady(false);
    lastSeekTimeRef.current = -1;
  }, [src]);

  const progressFraction = duration > 0 ? clamp(currentTime / duration, 0, 1) : 0;

  // Compute buffered end fraction (rightmost buffered point)
  let bufferedFraction = 0;
  if (buffered && buffered.length > 0 && duration > 0) {
    bufferedFraction = clamp(buffered.end(buffered.length - 1) / duration, 0, 1);
  }

  const isExpanded = hovered || dragging;

  const showPreview = (hovered || dragging) && duration > 0 && previewReady;

  return (
    <div
      ref={trackRef}
      className={styles.progressBar}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onMouseMove={handleMouseMove}
      onMouseDown={handleMouseDown}
    >
      <div className={`${styles.progressTrack} ${isExpanded ? styles.progressTrackExpanded : ''}`}>
        {/* Buffered range */}
        <div
          className={styles.progressBuffered}
          style={{ width: `${bufferedFraction * 100}%` }}
        />
        {/* Progress fill */}
        <div
          className={styles.progressFill}
          style={{ width: `${progressFraction * 100}%` }}
        />
      </div>

      {/* Frame preview thumbnail — canvas always mounted so ref is stable */}
      <div
        className={styles.progressPreview}
        style={{
          left: `${hoverFraction * 100}%`,
          display: showPreview ? undefined : 'none',
        }}
      >
        <canvas ref={previewCanvasRef} className={styles.progressPreviewCanvas} />
      </div>

      {/* Hover time tooltip */}
      {(hovered || dragging) && duration > 0 && (
        <div
          className={styles.progressHoverTime}
          style={{ left: `${hoverFraction * 100}%` }}
        >
          {formatTime(hoverFraction * duration)}
        </div>
      )}

      {/* Hidden preview video — always mounted when src is available */}
      {src && (
        <video
          ref={previewVideoRef}
          src={src}
          muted
          preload="auto"
          style={{ position: 'absolute', width: 0, height: 0, opacity: 0, pointerEvents: 'none' }}
          onLoadedData={() => setPreviewReady(true)}
        />
      )}
    </div>
  );
}

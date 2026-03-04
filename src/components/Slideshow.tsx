import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { IconPlayerPause, IconPlayerPlay, IconChevronLeft, IconChevronRight, IconX } from '@tabler/icons-react';
import { useGlobalKeydown } from '../hooks/useGlobalKeydown';
import { mediaFileUrl, mediaThumbnailUrl } from '../lib/mediaUrl';
import type { MasonryImageItem } from './image-grid/shared';
import classes from './Slideshow.module.css';

// ── Types ──────────────────────────────────────────────────────────────────

interface SlideshowProps {
  images: MasonryImageItem[];
  startIndex?: number;
  onClose: () => void;
}

const INTERVALS = [3, 5, 10, 30] as const;

// ── Component ──────────────────────────────────────────────────────────────

export function Slideshow({ images, startIndex = 0, onClose }: SlideshowProps) {
  const [index, setIndex] = useState(Math.min(startIndex, images.length - 1));
  const [playing, setPlaying] = useState(true);
  const [intervalSec, setIntervalSec] = useState(5);
  const [controlsVisible, setControlsVisible] = useState(true);
  const hideTimerRef = useRef<ReturnType<typeof setTimeout>>();
  const timerRef = useRef<ReturnType<typeof setTimeout>>();

  const image = images[index];

  // Auto-advance
  useEffect(() => {
    if (!playing || images.length <= 1) return;
    timerRef.current = setTimeout(() => {
      setIndex(prev => (prev + 1) % images.length);
    }, intervalSec * 1000);
    return () => clearTimeout(timerRef.current);
  }, [playing, index, intervalSec, images.length]);

  // Auto-hide controls
  const showControls = useCallback(() => {
    setControlsVisible(true);
    clearTimeout(hideTimerRef.current);
    hideTimerRef.current = setTimeout(() => setControlsVisible(false), 3000);
  }, []);

  useEffect(() => {
    showControls();
    return () => clearTimeout(hideTimerRef.current);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const goNext = useCallback(() => {
    setIndex(prev => (prev + 1) % images.length);
    showControls();
  }, [images.length, showControls]);

  const goPrev = useCallback(() => {
    setIndex(prev => (prev - 1 + images.length) % images.length);
    showControls();
  }, [images.length, showControls]);

  const togglePlay = useCallback(() => {
    setPlaying(p => !p);
    showControls();
  }, [showControls]);

  const cycleInterval = useCallback(() => {
    setIntervalSec(prev => {
      const idx = INTERVALS.indexOf(prev as typeof INTERVALS[number]);
      return INTERVALS[(idx + 1) % INTERVALS.length];
    });
    showControls();
  }, [showControls]);

  // Keyboard
  const onKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === 'Escape' || e.key === 'F5') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
      e.preventDefault();
      goNext();
      return;
    }
    if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
      e.preventDefault();
      goPrev();
      return;
    }
    if (e.key === ' ') {
      e.preventDefault();
      togglePlay();
    }
  }, [onClose, goNext, goPrev, togglePlay]);
  useGlobalKeydown(onKeyDown, true, { capture: true });

  if (!image) return null;

  const isVideo = image.mime.startsWith('video/');
  const src = mediaFileUrl(image.hash, image.mime);
  const thumbSrc = mediaThumbnailUrl(image.hash);

  return createPortal(
    <div className={`${classes.root} no-drag-region`} onMouseMove={showControls}>
      {/* Background */}
      <div className={classes.backdrop} />

      {/* Image */}
      <div className={classes.imageArea}>
        {isVideo ? (
          <video
            key={image.hash}
            src={src}
            autoPlay
            loop
            muted
            className={classes.media}
          />
        ) : (
          <>
            {/* Low-res thumb behind full image for instant display */}
            <img src={thumbSrc} alt="" className={classes.media} draggable={false} />
            <img
              key={image.hash}
              src={src}
              alt={image.name ?? ''}
              className={classes.media}
              draggable={false}
            />
          </>
        )}
      </div>

      {/* Controls overlay */}
      <div className={`${classes.controls} ${controlsVisible ? classes.controlsVisible : ''}`}>
        <div className={classes.controlBar}>
          <button className={classes.controlBtn} onClick={goPrev} title="Previous">
            <IconChevronLeft size={20} />
          </button>
          <button className={classes.controlBtn} onClick={togglePlay} title={playing ? 'Pause' : 'Play'}>
            {playing ? <IconPlayerPause size={20} /> : <IconPlayerPlay size={20} />}
          </button>
          <button className={classes.controlBtn} onClick={goNext} title="Next">
            <IconChevronRight size={20} />
          </button>
          <button className={classes.intervalBtn} onClick={cycleInterval} title="Change interval">
            {intervalSec}s
          </button>
          <div className={classes.counter}>
            {index + 1} / {images.length}
          </div>
          <button className={classes.controlBtn} onClick={onClose} title="Exit (Esc)">
            <IconX size={18} />
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

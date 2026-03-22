/**
 * StripView — purpose-built canvas renderer for collection browsing.
 * Uses ThumbnailPipeline directly for efficient image loading.
 * No selection, no hover previews, no context menus — view only.
 */
import { useEffect, useLayoutEffect, useRef, useState, useMemo, useCallback } from 'react';
import type { MediaItem } from '../../grid/shared';
import { toMasonryItem, isVideoMime } from '../../grid/shared';
import { mediaFileUrl } from '../../../shared/lib/mediaUrl';
import { VideoPlayer } from './VideoPlayer';
import { useThumbnailPipelineLifecycle } from '../../../shared/lib/canvas/useThumbnailPipelineLifecycle';
import { THUMBNAIL_PIPELINE_REVEAL_MS } from '../../../shared/lib/canvas/thumbnailPipelinePolicy';
import {
  classifyCanvasScrollPhase,
  resolveCanvasScrollDirection,
  createIdleCanvasScrollState,
  type CanvasScrollPhase,
  type CanvasScrollState,
} from '../../../shared/lib/canvas/scrollState';
import { useSettingsStore } from '../../../state/settingsStore';
import styles from './StripView.module.css';

const GAP = 12;
const BUFFER_ROWS = 1; // keep 1 row above + below loaded, evict everything else
const CORNER_RADIUS = 6;

interface LayoutEntry { x: number; y: number; w: number; h: number }

// Wide-image threshold: if an image at the target column width would be
// wider than this fraction of the full container, give it its own row.
const WIDE_THRESHOLD = 0.8;

/**
 * Fit-vertical layout: each image is viewportHeight tall. Pack images
 * left-to-right into rows until the next one wouldn't fit. If only one
 * image in a row, center it horizontally with empty space on sides.
 */
function computeFitVerticalLayout(
  images: Array<{ aspectRatio: number }>,
  containerWidth: number,
  viewportHeight: number,
  gap: number,
): { positions: LayoutEntry[]; totalHeight: number } {
  if (images.length === 0 || containerWidth <= 0 || viewportHeight <= 0) {
    return { positions: [], totalHeight: 0 };
  }

  const positions: LayoutEntry[] = new Array(images.length);
  let y = 0;
  let i = 0;

  while (i < images.length) {
    // Pack images into this row at viewportHeight tall
    const rowImages: number[] = [];
    let rowWidth = 0;

    while (i < images.length) {
      const ar = images[i].aspectRatio || 1.5;
      const imgWidth = viewportHeight * ar;
      const needed = rowImages.length > 0 ? imgWidth + gap : imgWidth;

      if (rowImages.length > 0 && rowWidth + needed > containerWidth) break;

      rowImages.push(i);
      rowWidth += needed;
      i++;
    }

    // Position images in this row, centered horizontally as a group
    const totalRowWidth = rowWidth;
    let x = Math.max(0, (containerWidth - totalRowWidth) / 2);

    for (const idx of rowImages) {
      const ar = images[idx].aspectRatio || 1.5;
      const imgWidth = viewportHeight * ar;
      // Cap width to container if a single very wide image
      const w = Math.min(imgWidth, containerWidth);
      const h = w / ar;
      const yOffset = (viewportHeight - h) / 2;
      positions[idx] = { x, y: y + yOffset, w, h };
      x += w + gap;
    }

    y += viewportHeight + gap;
  }

  return { positions, totalHeight: Math.max(0, y - gap) };
}

function computeStripLayout(
  images: Array<{ aspectRatio: number }>,
  containerWidth: number,
  cols: number,
  gap: number,
): { positions: LayoutEntry[]; totalHeight: number } {
  if (images.length === 0 || containerWidth <= 0) return { positions: [], totalHeight: 0 };

  // Single column: simple vertical stack, each image fills full width
  if (cols === 1) {
    const positions: LayoutEntry[] = [];
    let y = 0;
    for (let i = 0; i < images.length; i++) {
      const ar = images[i].aspectRatio || 1.5;
      const h = containerWidth / ar;
      positions.push({ x: 0, y, w: containerWidth, h });
      y += h + gap;
    }
    return { positions, totalHeight: Math.max(0, y - gap) };
  }

  // Multi-column: row-based packing with wide-image detection.
  // Images wider than WIDE_THRESHOLD of the container get their own row.
  // Images sharing a row use the tallest image's height; shorter ones
  // are centered vertically within the row.
  const positions: LayoutEntry[] = new Array(images.length);
  let y = 0;
  let i = 0;

  while (i < images.length) {
    const ar = images[i].aspectRatio || 1.5;

    // Check if this image is "wide" — its natural width at colWidth height
    // would exceed WIDE_THRESHOLD of the container.
    // At colWidth, image height = colWidth / ar. Its natural display width = colWidth.
    // But we care about aspect ratio: a landscape image (ar > 1) is wide.
    // "Wide" means: if we render it at colWidth, it's still very wide relative
    // to the container. More precisely: ar * rowHeight would fill > 80% of container.
    // Simpler: if ar >= cols * WIDE_THRESHOLD, one image fills most of the row.
    const isWide = ar >= cols * WIDE_THRESHOLD;

    if (isWide) {
      // Full-width row for this image
      const h = containerWidth / ar;
      positions[i] = { x: 0, y, w: containerWidth, h };
      y += h + gap;
      i++;
      continue;
    }

    // Pack up to `cols` images into this row
    const rowImages: number[] = [];
    while (rowImages.length < cols && i < images.length) {
      const nextAr = images[i].aspectRatio || 1.5;
      // Don't pack a wide image into a shared row
      if (nextAr >= cols * WIDE_THRESHOLD && rowImages.length > 0) break;
      rowImages.push(i);
      i++;
    }

    const count = rowImages.length;
    const totalGap = (count - 1) * gap;
    const itemWidth = Math.floor((containerWidth - totalGap) / count);

    // Row height = tallest image in the row
    let rowHeight = 0;
    for (const idx of rowImages) {
      const imgAr = images[idx].aspectRatio || 1.5;
      const h = itemWidth / imgAr;
      if (h > rowHeight) rowHeight = h;
    }

    // Position each image centered vertically within the row height
    let x = 0;
    for (const idx of rowImages) {
      const imgAr = images[idx].aspectRatio || 1.5;
      const imgH = itemWidth / imgAr;
      const yOffset = (rowHeight - imgH) / 2;
      positions[idx] = { x, y: y + yOffset, w: itemWidth, h: imgH };
      x += itemWidth + gap;
    }

    y += rowHeight + gap;
  }

  return { positions, totalHeight: Math.max(0, y - gap) };
}

export type StripFitMode = 'horizontal' | 'vertical';

interface StripViewProps {
  images: MediaItem[];
  initialIndex: number;
  cols?: number;
  fitMode?: StripFitMode;
  resetKey?: number;
  onLoadMore?: () => void;
}

export function StripView({
  images,
  initialIndex,
  cols = 1,
  fitMode = 'horizontal',
  resetKey = 0,
  onLoadMore,
}: StripViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null);
  const initialScrollDone = useRef(false);
  const drawRafRef = useRef(0);
  const scrollTopRef = useRef(0);
  const [viewportHeight, setViewportHeight] = useState(800);
  const viewportHeightRef = useRef(800);
  const [activeVideo, setActiveVideo] = useState<number | null>(null);
  const activeVideoRef = useRef<number | null>(null);
  activeVideoRef.current = activeVideo;

  const masonryImages = useMemo(() => images.map(toMasonryItem), [images]);

  // ─── Container measurement ──────────────────────────────────
  const [containerWidth, setContainerWidth] = useState(0);
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el) {
      if (el.clientWidth > 0) setContainerWidth(el.clientWidth);
      if (el.clientHeight > 0) {
        setViewportHeight(el.clientHeight);
        viewportHeightRef.current = el.clientHeight;
      }
    }
  }, []);
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => setContainerWidth(Math.round(entry.contentRect.width)));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // ─── Layout ──────────────────────────────────────────────────
  // Before recomputing layout on cols change, find the image at viewport center
  // so we can restore scroll position after layout changes.
  const prevColsRef = useRef(cols);
  const centerImageIndexRef = useRef(0);
  const positionsRef = useRef<LayoutEntry[]>([]);

  // Snapshot center image before layout recomputes
  if (cols !== prevColsRef.current && positionsRef.current.length > 0) {
    const st = scrollTopRef.current;
    const vh = viewportHeightRef.current;
    const centerY = st + vh / 2;
    let bestIdx = 0;
    let bestDist = Infinity;
    for (let i = 0; i < positionsRef.current.length; i++) {
      const pos = positionsRef.current[i];
      const mid = pos.y + pos.h / 2;
      const dist = Math.abs(mid - centerY);
      if (dist < bestDist) { bestDist = dist; bestIdx = i; }
    }
    centerImageIndexRef.current = bestIdx;
    prevColsRef.current = cols;
  }

  const { positions, totalHeight } = useMemo(
    () => fitMode === 'vertical'
      ? computeFitVerticalLayout(masonryImages, containerWidth, viewportHeight - GAP * 2, GAP)
      : computeStripLayout(masonryImages, containerWidth, cols, GAP),
    [masonryImages, containerWidth, cols, fitMode, viewportHeight],
  );
  positionsRef.current = positions;

  // ─── Scroll state ───────────────────────────────────────────
  const scrollPhaseRef = useRef<CanvasScrollPhase>('idle');
  const scrollStateRef = useRef<CanvasScrollState>(createIdleCanvasScrollState());
  const prevScrollTopRef = useRef(0);
  const prevScrollTimeRef = useRef(0);
  const pendingAtlasDirtyRef = useRef(false);

  // ─── Pipeline ────────────────────────────────────────────────
  // Use a ref for the draw function so markDirty always calls the latest version
  const drawRef = useRef<() => void>(() => {});

  const markDirty = useCallback(() => {
    if (drawRafRef.current) return;
    drawRafRef.current = requestAnimationFrame(() => {
      drawRafRef.current = 0;
      drawRef.current();
    });
  }, []);

  const atlasRef = useThumbnailPipelineLifecycle({
    markDirty,
    scrollPhaseRef,
    pendingAtlasDirtyRef,
  });

  // ─── Draw function ──────────────────────────────────────────
  const drawFn = useCallback(() => {
    const canvas = canvasRef.current;
    const el = scrollRef.current;
    if (!canvas || !el) return;

    const dpr = window.devicePixelRatio || 1;
    const cssW = el.clientWidth;
    const cssH = el.clientHeight;
    const pxW = Math.round(cssW * dpr);
    const pxH = Math.round(cssH * dpr);

    if (canvas.width !== pxW || canvas.height !== pxH) {
      canvas.width = pxW;
      canvas.height = pxH;
      canvas.style.width = `${cssW}px`;
      canvas.style.height = `${cssH}px`;
      ctxRef.current = canvas.getContext('2d', { alpha: false });
    }

    const ctx = ctxRef.current;
    if (!ctx) return;

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    // Background
    const bg = getComputedStyle(canvas).getPropertyValue('--color-theme').trim() || '#1a1a1e';
    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, cssW, cssH);

    const atlas = atlasRef.current;
    if (!atlas) return;
    atlas.setScrollState(scrollStateRef.current);

    const st = scrollTopRef.current;
    const vh = viewportHeightRef.current;
    const bufferPx = vh * BUFFER_ROWS;
    const visTop = st - bufferPx;
    const visBottom = st + vh + bufferPx;
    const now = performance.now();
    let hasActiveReveal = false;

    for (let i = 0; i < positions.length; i++) {
      const pos = positions[i];
      if (pos.y + pos.h < visTop) continue;
      if (pos.y > visBottom) break;

      const img = masonryImages[i];
      if (!img) continue;

      const drawY = pos.y - st;

      // Request image from pipeline
      atlas.ensure(img.hash, {
        y: pos.y,
        drawWidth: pos.w * dpr,
        drawHeight: pos.h * dpr,
        mime: img.mime,
        sourceWidth: img.width,
        sourceHeight: img.height,
      });

      const entry = atlas.get(img.hash);
      const r = CORNER_RADIUS;

      if (entry?.thumb) {
        let alpha = 1;
        if (entry.animateIn && entry.revealStartedAt > 0) {
          const elapsed = now - entry.revealStartedAt;
          alpha = Math.min(1, elapsed / THUMBNAIL_PIPELINE_REVEAL_MS);
          if (alpha < 1) hasActiveReveal = true;
        }

        // Placeholder behind during fade-in
        if (alpha < 1) {
          ctx.beginPath();
          ctx.roundRect(pos.x, drawY, pos.w, pos.h, r);
          ctx.fillStyle = img.dominant_color_hex || '#2a2a2e';
          ctx.fill();
        }

        // Image with rounded corners
        ctx.save();
        ctx.beginPath();
        ctx.roundRect(pos.x, drawY, pos.w, pos.h, r);
        ctx.clip();
        ctx.globalAlpha = alpha;
        ctx.drawImage(entry.thumb, pos.x, drawY, pos.w, pos.h);
        ctx.globalAlpha = 1;
        ctx.restore();

        // Glass border: light top + sides, dark bottom
        ctx.beginPath();
        ctx.roundRect(pos.x + 0.5, drawY + 0.5, pos.w - 1, pos.h - 1, r);
        ctx.strokeStyle = 'rgba(255, 255, 255, 0.08)';
        ctx.lineWidth = 1;
        ctx.stroke();

        // Play button for videos
        if (isVideoMime(img.mime) && activeVideoRef.current !== i) {
          const btnSize = Math.min(48, pos.w * 0.15, pos.h * 0.15);
          const cx = pos.x + pos.w / 2;
          const cy = drawY + pos.h / 2;
          ctx.beginPath();
          ctx.arc(cx, cy, btnSize, 0, Math.PI * 2);
          ctx.fillStyle = 'rgba(0, 0, 0, 0.5)';
          ctx.fill();
          ctx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
          ctx.lineWidth = 1.5;
          ctx.stroke();
          // Triangle
          const triSize = btnSize * 0.45;
          const triX = cx - triSize * 0.35;
          ctx.beginPath();
          ctx.moveTo(triX, cy - triSize);
          ctx.lineTo(triX + triSize * 1.2, cy);
          ctx.lineTo(triX, cy + triSize);
          ctx.closePath();
          ctx.fillStyle = 'rgba(255, 255, 255, 0.9)';
          ctx.fill();
        }
      } else {
        // Placeholder with rounded corners
        ctx.beginPath();
        ctx.roundRect(pos.x, drawY, pos.w, pos.h, r);
        ctx.fillStyle = img.dominant_color_hex || '#2a2a2e';
        ctx.fill();

        // Glass border on placeholder too
        ctx.beginPath();
        ctx.roundRect(pos.x + 0.5, drawY + 0.5, pos.w - 1, pos.h - 1, r);
        ctx.strokeStyle = 'rgba(255, 255, 255, 0.06)';
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    }

    if (hasActiveReveal) markDirty();
  }, [atlasRef, markDirty, masonryImages, positions]);

  // Keep drawRef current
  drawRef.current = drawFn;

  // ─── Scroll handler ─────────────────────────────────────────
  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;

    const now = performance.now();
    const prevTop = prevScrollTopRef.current;
    const dt = now - prevScrollTimeRef.current;
    const dy = Math.abs(el.scrollTop - prevTop);
    const velocity = dt > 0 ? (dy / dt) * 1000 : 0;

    prevScrollTopRef.current = el.scrollTop;
    prevScrollTimeRef.current = now;
    scrollTopRef.current = el.scrollTop;
    const newVh = el.clientHeight;
    if (newVh !== viewportHeightRef.current) {
      viewportHeightRef.current = newVh;
      setViewportHeight(newVh);
    }

    const phase = classifyCanvasScrollPhase(velocity);
    const direction = resolveCanvasScrollDirection(el.scrollTop - prevTop);
    scrollPhaseRef.current = phase;
    scrollStateRef.current = { phase, direction, velocityPxPerSec: velocity };

    markDirty();

    if (onLoadMore) {
      const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
      if (dist < 2000) onLoadMore();
    }
  }, [markDirty, onLoadMore]);

  // Redraw on layout changes + restore scroll center on cols change
  useEffect(() => {
    const el = scrollRef.current;
    if (el && positions.length > 0 && centerImageIndexRef.current > 0) {
      const idx = Math.min(centerImageIndexRef.current, positions.length - 1);
      const pos = positions[idx];
      if (pos) {
        const target = pos.y + pos.h / 2 - el.clientHeight / 2;
        el.scrollTop = Math.max(0, target);
        scrollTopRef.current = el.scrollTop;
      }
    }
    markDirty();
  }, [markDirty, positions, cols]);

  // ─── Reset scroll on navigation ─────────────────────────────
  const prevResetKey = useRef(resetKey);
  useEffect(() => {
    if (resetKey !== prevResetKey.current) {
      prevResetKey.current = resetKey;
      const el = scrollRef.current;
      if (el) {
        el.scrollTop = 0;
        scrollTopRef.current = 0;
      }
      initialScrollDone.current = false;
    }
  }, [resetKey]);

  // ─── Initial scroll to index ────────────────────────────────
  useEffect(() => {
    if (initialScrollDone.current || images.length === 0 || positions.length === 0) return;
    initialScrollDone.current = true;
    if (initialIndex <= 0) return;
    const el = scrollRef.current;
    if (!el) return;
    const idx = Math.min(initialIndex, positions.length - 1);
    const pos = positions[idx];
    if (pos) {
      el.scrollTop = Math.max(0, pos.y - el.clientHeight / 2 + pos.h / 2);
      scrollTopRef.current = el.scrollTop;
    }
  }, [images.length, positions, initialIndex]);

  // ─── Keyboard scrolling ─────────────────────────────────────
  const stripScrollSpeed = useSettingsStore(s => s.settings.stripScrollSpeed);
  const stripScrollEnabled = useSettingsStore(s => s.settings.stripScrollEnabled);

  useEffect(() => {
    if (!stripScrollEnabled) return;

    const scrollDir = { current: 0 };
    let rafId = 0;
    let holdStart = 0;
    const ACCEL_MS = 400;
    const MIN_PX = 6;
    const maxPx = stripScrollSpeed;

    const tick = () => {
      if (scrollDir.current !== 0 && scrollRef.current) {
        const t = Math.min(1, (performance.now() - holdStart) / ACCEL_MS);
        const speed = MIN_PX + (maxPx - MIN_PX) * (1 - (1 - t) * (1 - t));
        scrollRef.current.scrollBy({ top: scrollDir.current * speed, behavior: 'instant' });
      }
      if (scrollDir.current !== 0) rafId = requestAnimationFrame(tick);
    };

    const onDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const dir = e.key === 'ArrowUp' || e.key === 'w' ? -1 : e.key === 'ArrowDown' || e.key === 's' ? 1 : 0;
      if (!dir || e.repeat) return;
      e.preventDefault();
      if (scrollDir.current === 0) { holdStart = performance.now(); scrollDir.current = dir; rafId = requestAnimationFrame(tick); }
    };

    const onUp = (e: KeyboardEvent) => {
      const dir = e.key === 'ArrowUp' || e.key === 'w' ? -1 : e.key === 'ArrowDown' || e.key === 's' ? 1 : 0;
      if (dir && scrollDir.current === dir) { scrollDir.current = 0; cancelAnimationFrame(rafId); }
    };

    window.addEventListener('keydown', onDown);
    window.addEventListener('keyup', onUp);
    return () => { window.removeEventListener('keydown', onDown); window.removeEventListener('keyup', onUp); cancelAnimationFrame(rafId); };
  }, [stripScrollEnabled, stripScrollSpeed]);

  // ─── Click to play video ──────────────────────────────────
  const handleClick = useCallback((e: React.MouseEvent) => {
    const el = scrollRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const clickX = e.clientX - rect.left + el.scrollLeft;
    const clickY = e.clientY - rect.top + el.scrollTop;

    for (let i = 0; i < positions.length; i++) {
      const pos = positions[i];
      if (clickX >= pos.x && clickX <= pos.x + pos.w && clickY >= pos.y && clickY <= pos.y + pos.h) {
        const img = masonryImages[i];
        if (img && isVideoMime(img.mime)) {
          setActiveVideo(activeVideo === i ? null : i);
        } else {
          setActiveVideo(null);
        }
        return;
      }
    }
    setActiveVideo(null);
  }, [activeVideo, masonryImages, positions]);

  // Dismiss video when scrolling far from it
  useEffect(() => {
    if (activeVideo == null || !positions[activeVideo]) return;
    const pos = positions[activeVideo];
    const st = scrollTopRef.current;
    const vh = viewportHeightRef.current;
    if (pos.y + pos.h < st - vh || pos.y > st + vh * 2) {
      setActiveVideo(null);
    }
  });

  // Video overlay position
  const videoSettings = useSettingsStore(s => s.settings);
  const activePos = activeVideo != null ? positions[activeVideo] : null;
  const activeImg = activeVideo != null ? masonryImages[activeVideo] : null;

  return (
    <div className={styles.stripView}>
      <div ref={scrollRef} className={styles.scrollContainer} onScroll={handleScroll} onClick={handleClick}>
        <div style={{ height: totalHeight, position: 'relative' }}>
          <canvas
            ref={canvasRef}
            style={{ position: 'sticky', top: 0, display: 'block', pointerEvents: 'none' }}
          />
          {activeVideo != null && activePos && activeImg && (
            <div
              style={{
                position: 'absolute',
                top: activePos.y,
                left: activePos.x,
                width: activePos.w,
                height: activePos.h,
                borderRadius: CORNER_RADIUS,
                overflow: 'hidden',
                zIndex: 1,
              }}
              onClick={(e) => e.stopPropagation()}
            >
              <VideoPlayer
                src={mediaFileUrl(activeImg.hash, activeImg.mime)}
                autoPlay={videoSettings.videoAutoPlay}
                loop={videoSettings.videoLoop}
                muted={videoSettings.videoMuted}
                initialVolume={videoSettings.videoVolume}
                initialPlaybackRate={videoSettings.videoPlaybackRate}
                onVolumeChange={(v) => useSettingsStore.getState().updateSetting('videoVolume', v)}
                onMutedChange={(m) => useSettingsStore.getState().updateSetting('videoMuted', m)}
                onPlaybackRateChange={(r) => useSettingsStore.getState().updateSetting('videoPlaybackRate', r)}
                onLoopChange={(l) => useSettingsStore.getState().updateSetting('videoLoop', l)}
              />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

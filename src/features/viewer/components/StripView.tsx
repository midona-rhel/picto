/**
 * StripView — purpose-built canvas renderer for collection browsing.
 * Uses ThumbnailPipeline directly for efficient image loading.
 * No selection, no hover previews, no context menus — view only.
 */
import { useEffect, useLayoutEffect, useRef, useState, useMemo, useCallback } from 'react';
import type { MediaItem } from '../../grid/shared';
import { toMasonryItem } from '../../grid/shared';
import { ThumbnailPipeline } from '../../../shared/lib/canvas/thumbnailPipeline';
import { useThumbnailPipelineLifecycle } from '../../../shared/lib/canvas/useThumbnailPipelineLifecycle';
import { THUMBNAIL_PIPELINE_REVEAL_MS } from '../../../shared/lib/canvas/thumbnailPipelinePolicy';
import { classifyCanvasScrollPhase, resolveCanvasScrollDirection, createIdleCanvasScrollState, type CanvasScrollPhase } from '../../../shared/lib/canvas/scrollState';
import styles from './StripView.module.css';

const GAP = 12;
const BUFFER_ROWS = 2; // render this many extra rows above/below viewport

interface LayoutEntry { x: number; y: number; w: number; h: number }

function computeStripLayout(
  images: Array<{ aspectRatio: number }>,
  containerWidth: number,
  cols: number,
  gap: number,
): { positions: LayoutEntry[]; totalHeight: number } {
  if (images.length === 0 || containerWidth <= 0) return { positions: [], totalHeight: 0 };
  const colWidth = Math.floor((containerWidth - (cols - 1) * gap) / cols);
  const positions: LayoutEntry[] = [];
  const colHeights = new Float64Array(cols);

  for (let i = 0; i < images.length; i++) {
    const col = i % cols;
    if (cols === 1) {
      // Single column: simple vertical stack
      const ar = images[i].aspectRatio || 1.5;
      const h = colWidth / ar;
      const y = i === 0 ? 0 : positions[i - 1].y + positions[i - 1].h + gap;
      positions.push({ x: 0, y, w: colWidth, h });
    } else {
      // Multi-column: place in shortest column
      let shortest = 0;
      for (let c = 1; c < cols; c++) {
        if (colHeights[c] < colHeights[shortest]) shortest = c;
      }
      const ar = images[i].aspectRatio || 1.5;
      const h = colWidth / ar;
      const x = shortest * (colWidth + gap);
      const y = colHeights[shortest];
      positions.push({ x, y, w: colWidth, h });
      colHeights[shortest] = y + h + gap;
    }
  }

  let totalHeight = 0;
  if (cols === 1 && positions.length > 0) {
    const last = positions[positions.length - 1];
    totalHeight = last.y + last.h;
  } else {
    for (let c = 0; c < cols; c++) {
      if (colHeights[c] > totalHeight) totalHeight = colHeights[c];
    }
    if (totalHeight > 0) totalHeight -= gap;
  }

  return { positions, totalHeight };
}

interface StripViewProps {
  images: MediaItem[];
  initialIndex: number;
  cols?: number;
  resetKey?: number;
  onLoadMore?: () => void;
}

export function StripView({
  images,
  initialIndex,
  cols = 1,
  resetKey = 0,
  onLoadMore,
}: StripViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null);
  const initialScrollDone = useRef(false);
  const drawScheduled = useRef(false);

  const masonryImages = useMemo(() => images.map(toMasonryItem), [images]);

  // ─── Container measurement ──────────────────────────────────
  const [containerWidth, setContainerWidth] = useState(0);
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && el.clientWidth > 0) setContainerWidth(el.clientWidth);
  }, []);
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => setContainerWidth(Math.round(entry.contentRect.width)));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // ─── Layout ──────────────────────────────────────────────────
  const { positions, totalHeight } = useMemo(
    () => computeStripLayout(masonryImages, containerWidth, cols, GAP),
    [masonryImages, containerWidth, cols],
  );

  // ─── Scroll state ───────────────────────────────────────────
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(800);
  const scrollPhaseRef = useRef<CanvasScrollPhase>('idle');
  const scrollStateRef = useRef(createIdleCanvasScrollState());
  const pendingAtlasDirtyRef = useRef(false);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    setScrollTop(el.scrollTop);
    setViewportHeight(el.clientHeight);

    // Update scroll phase for pipeline
    const now = performance.now();
    const prev = scrollStateRef.current;
    const direction = resolveCanvasScrollDirection(el.scrollTop, prev.scrollTop);
    const next = classifyCanvasScrollPhase(el.scrollTop, prev, now);
    scrollStateRef.current = { ...next, scrollTop: el.scrollTop };
    scrollPhaseRef.current = next.phase;

    // Load more near bottom
    if (onLoadMore) {
      const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
      if (dist < 2000) onLoadMore();
    }
  }, [onLoadMore]);

  // ─── Pipeline ────────────────────────────────────────────────
  const markDirty = useCallback((_lanes: 'base' | 'overlay' | 'both') => {
    if (drawScheduled.current) return;
    drawScheduled.current = true;
    requestAnimationFrame(() => {
      drawScheduled.current = false;
      draw();
    });
  }, []);

  const atlasRef = useThumbnailPipelineLifecycle({
    markDirty,
    scrollPhaseRef,
    pendingAtlasDirtyRef,
  });

  // ─── Draw ────────────────────────────────────────────────────
  const draw = useCallback(() => {
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

    // Fill background
    ctx.fillStyle = getComputedStyle(canvas).getPropertyValue('--color-theme').trim() || '#1a1a1e';
    ctx.fillRect(0, 0, cssW, cssH);

    const atlas = atlasRef.current;
    if (!atlas) return;

    // Update pipeline scroll state
    atlas.setScrollState(scrollStateRef.current);

    const st = el.scrollTop;
    const bufferPx = viewportHeight * BUFFER_ROWS;
    const visTop = st - bufferPx;
    const visBottom = st + cssH + bufferPx;
    const now = performance.now();
    let hasActiveReveal = false;

    for (let i = 0; i < positions.length; i++) {
      const pos = positions[i];
      if (pos.y + pos.h < visTop) continue;
      if (pos.y > visBottom) break;

      const img = masonryImages[i];
      if (!img) continue;

      const drawY = pos.y - st;

      // Ensure pipeline is loading this image
      atlas.ensure(img.hash, {
        y: pos.y,
        drawWidth: pos.w * dpr,
        drawHeight: pos.h * dpr,
        mime: img.mime,
        sourceWidth: img.width,
        sourceHeight: img.height,
      });

      const entry = atlas.get(img.hash);

      if (entry?.thumb) {
        // Fade-in animation
        let alpha = 1;
        if (entry.animateIn && entry.revealStartedAt > 0) {
          const elapsed = now - entry.revealStartedAt;
          alpha = Math.min(1, elapsed / THUMBNAIL_PIPELINE_REVEAL_MS);
          if (alpha < 1) hasActiveReveal = true;
        }

        // Draw placeholder behind if fading in
        if (alpha < 1) {
          ctx.fillStyle = img.dominant_color_hex || '#2a2a2e';
          ctx.fillRect(pos.x, drawY, pos.w, pos.h);
        }

        ctx.globalAlpha = alpha;
        ctx.drawImage(entry.thumb, pos.x, drawY, pos.w, pos.h);
        ctx.globalAlpha = 1;
      } else {
        // Placeholder
        ctx.fillStyle = img.dominant_color_hex || '#2a2a2e';
        ctx.fillRect(pos.x, drawY, pos.w, pos.h);
      }
    }

    // Continue animation if reveals are active
    if (hasActiveReveal) {
      markDirty('base');
    }
  }, [atlasRef, containerWidth, markDirty, masonryImages, positions, viewportHeight]);

  // Redraw on scroll/layout changes
  useEffect(() => { draw(); }, [draw, scrollTop, containerWidth, cols, positions]);

  // ─── Reset scroll on navigation ─────────────────────────────
  const prevResetKey = useRef(resetKey);
  useEffect(() => {
    if (resetKey !== prevResetKey.current) {
      prevResetKey.current = resetKey;
      const el = scrollRef.current;
      if (el) el.scrollTop = 0;
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
    }
  }, [images.length, positions, initialIndex]);

  // ─── Keyboard scrolling ─────────────────────────────────────
  useEffect(() => {
    const scrollDir = { current: 0 };
    let rafId = 0;
    let holdStartTime = 0;
    const ACCEL_MS = 600;
    const MAX_PX = 36;
    const MIN_PX = 4;

    const tick = () => {
      if (scrollDir.current !== 0 && scrollRef.current) {
        const t = Math.min(1, (performance.now() - holdStartTime) / ACCEL_MS);
        const speed = MIN_PX + (MAX_PX - MIN_PX) * (1 - (1 - t) * (1 - t));
        scrollRef.current.scrollBy({ top: scrollDir.current * speed, behavior: 'instant' });
      }
      if (scrollDir.current !== 0) rafId = requestAnimationFrame(tick);
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const dir = e.key === 'ArrowUp' || e.key === 'w' ? -1 : e.key === 'ArrowDown' || e.key === 's' ? 1 : 0;
      if (!dir) return;
      e.preventDefault();
      if (e.repeat) return;
      if (scrollDir.current === 0) {
        holdStartTime = performance.now();
        scrollDir.current = dir;
        rafId = requestAnimationFrame(tick);
      }
    };

    const onKeyUp = (e: KeyboardEvent) => {
      const dir = e.key === 'ArrowUp' || e.key === 'w' ? -1 : e.key === 'ArrowDown' || e.key === 's' ? 1 : 0;
      if (dir && scrollDir.current === dir) {
        scrollDir.current = 0;
        cancelAnimationFrame(rafId);
      }
    };

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      cancelAnimationFrame(rafId);
    };
  }, []);

  // ─── Render ──────────────────────────────────────────────────
  return (
    <div className={styles.stripView}>
      <div ref={scrollRef} className={styles.scrollContainer} onScroll={handleScroll}>
        <div style={{ height: totalHeight, position: 'relative' }}>
          <canvas
            ref={canvasRef}
            style={{
              position: 'sticky',
              top: 0,
              display: 'block',
              pointerEvents: 'none',
            }}
          />
        </div>
      </div>
    </div>
  );
}

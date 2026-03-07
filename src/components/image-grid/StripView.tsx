/**
 * StripView — vertical strip renderer for collection members.
 * Rendered inside DetailView's content area when the current image is a collection.
 * All navigation (left/right, escape, enter, rating) is handled by DetailView.
 * StripView only handles vertical scrolling (W/S/Up/Down).
 */
import { useEffect, useLayoutEffect, useRef, useState, useCallback, useMemo } from 'react';
import { mediaThumbnailUrl, mediaFileUrl } from '../../shared/lib/mediaUrl';
import { queueImageDecode } from './useImagePreloader';
import type { MasonryImageItem } from './shared';
import { isVideoMime } from './shared';
import styles from './StripView.module.css';

const GAP = 12;
const OVERSCAN = 2000;
const SCROLL_STEP = 200;

interface StripViewProps {
  images: MasonryImageItem[];
  initialIndex: number;
  zoomScale?: number;
  resetKey?: number;
  onLoadMore?: () => void;
}

interface LayoutEntry {
  offsetY: number;
  height: number;
  width: number;
}

function computeLayout(images: MasonryImageItem[], containerWidth: number, scale: number): LayoutEntry[] {
  const layout: LayoutEntry[] = [];
  const contentWidth = containerWidth * scale;
  let y = 0;
  for (const img of images) {
    const h = contentWidth / img.aspectRatio;
    layout.push({ offsetY: y, height: h, width: contentWidth });
    y += h + GAP;
  }
  return layout;
}

export function StripView({
  images,
  initialIndex,
  zoomScale = 1,
  resetKey = 0,
  onLoadMore,
}: StripViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState(800);
  const initialScrollDone = useRef(false);

  // ─── Container width measurement ──────────────────────────
  // Synchronous measurement on mount so the first paint uses the correct width
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && el.clientWidth > 0) setContainerWidth(el.clientWidth);
  }, []);
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 800;
      setContainerWidth(w);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // ─── Layout computation ────────────────────────────────────
  const layout = useMemo(() => computeLayout(images, containerWidth, zoomScale), [images, containerWidth, zoomScale]);
  const totalHeight = useMemo(() => {
    if (layout.length === 0) return 0;
    const last = layout[layout.length - 1];
    return last.offsetY + last.height;
  }, [layout]);

  // ─── Visible range (virtual scroll) ───────────────────────
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(800);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    setScrollTop(el.scrollTop);
    setViewportHeight(el.clientHeight);

    // Load more pages near bottom
    if (onLoadMore) {
      const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
      if (distFromBottom < 2000) {
        onLoadMore();
      }
    }
  }, [onLoadMore]);

  // Visible range
  const visStart = scrollTop - OVERSCAN;
  const visEnd = scrollTop + viewportHeight + OVERSCAN;
  const visibleIndices: number[] = [];
  for (let i = 0; i < layout.length; i++) {
    const entry = layout[i];
    if (entry.offsetY + entry.height < visStart) continue;
    if (entry.offsetY > visEnd) break;
    visibleIndices.push(i);
  }

  // Reset scroll when resetKey changes (navigation, fit-to-window)
  const prevResetKey = useRef(resetKey);
  useEffect(() => {
    if (resetKey !== prevResetKey.current) {
      prevResetKey.current = resetKey;
      const el = scrollRef.current;
      if (el) el.scrollTop = 0;
      initialScrollDone.current = false;
    }
  }, [resetKey]);

  // Initial scroll to initialIndex
  useEffect(() => {
    if (!initialScrollDone.current && images.length > 0 && layout.length > 0) {
      const idx = Math.min(initialIndex, images.length - 1);
      const el = scrollRef.current;
      if (el && layout[idx]) {
        const entry = layout[idx];
        const target = entry.offsetY - (el.clientHeight - entry.height) / 2;
        el.scrollTop = Math.max(0, target);
      }
      initialScrollDone.current = true;
    }
  }, [images.length, layout, initialIndex]);

  // ─── Keyboard: only vertical scrolling ─────────────────────
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      switch (e.key) {
        case 'w':
        case 'ArrowUp': {
          e.preventDefault();
          scrollRef.current?.scrollBy({ top: -SCROLL_STEP, behavior: 'smooth' });
          break;
        }
        case 's':
        case 'ArrowDown': {
          e.preventDefault();
          scrollRef.current?.scrollBy({ top: SCROLL_STEP, behavior: 'smooth' });
          break;
        }
      }
    };

    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, []);

  // ─── Render ────────────────────────────────────────────────
  const contentWidth = containerWidth * zoomScale;

  return (
    <div className={styles.stripView}>
      <div
        ref={scrollRef}
        className={styles.scrollContainer}
        onScroll={handleScroll}
      >
        <div className={styles.spacer} style={{ height: totalHeight, width: contentWidth }}>
          {visibleIndices.map((i) => {
            const entry = layout[i];
            const img = images[i];
            return (
              <StripImageSlot
                key={img.hash}
                image={img}
                offsetY={entry.offsetY}
                height={entry.height}
                width={entry.width}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ─── Individual image slot with thumbnail → full decode ──────

interface StripImageSlotProps {
  image: MasonryImageItem;
  offsetY: number;
  height: number;
  width: number;
}

function StripImageSlot({ image, offsetY, height, width }: StripImageSlotProps) {
  const [src, setSrc] = useState(() => mediaThumbnailUrl(image.hash));

  useEffect(() => {
    if (isVideoMime(image.mime)) return;

    const fullUrl = mediaFileUrl(image.hash, image.mime);
    const cancel = queueImageDecode(fullUrl, (url) => {
      setSrc(url);
    }, 'high');

    return cancel;
  }, [image.hash, image.mime]);

  return (
    <div
      className={styles.imageSlot}
      style={{
        top: offsetY,
        height,
        width,
      }}
    >
      <img
        src={src}
        alt=""
        draggable={false}
        style={{ height: '100%', objectFit: 'contain' }}
      />
    </div>
  );
}

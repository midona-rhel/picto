/**
 * Canvas base layer — images, placeholders, badges, text.
 *
 * Receives activeTiles[] from CanvasGrid and draws only those tiles.
 * Multi-pass: images+reveal → glass borders → badges → stars → text.
 */

import type { LayoutItem, GridViewMode } from '../layout/types';
import type { CanvasRenderItem } from './renderItemAdapter';
import type { ThumbnailPipelineEntry } from './thumbnailPipeline';
import {
  BADGE_H,
  INFO_BASELINE,
  INFO_FONT,
  NAME_BASELINE,
  NAME_FONT,
  drawBadge,
  drawGroupBadge,
  drawImageContain,
  drawImageCover,
  getContainRect,
  formatLabelForMime,
  mimeToExt,
  truncateText,
  formatDuration,
} from './primitives';
import { GRID_BADGE_INSET } from '../gridAppearance';
import { drawBrokenThumbnail } from '../../../shared/ui/ThumbnailImage/drawBrokenThumbnail';
import { drawFontThumbnail } from '../../../shared/ui/ThumbnailImage/drawFontThumbnail';


interface ThemeLike {
  placeholderBg: string;
  isLight: boolean;
  borderRadius: number;
  textPrimary: string;
  textTertiary: string;
  glassBorder: string;
}

export interface DrawContext {
  scrollTop: number;
  viewportHeight: number;
  textHeight: number;
  borderRadius: number;
}

export interface BaseLayerArgs {
  ctx: CanvasRenderingContext2D;
  positions: LayoutItem[];
  items: CanvasRenderItem[];
  atlasGet: (fileHash: string) => ThumbnailPipelineEntry | null;
  revealProgress: (entityHash: string) => number;
  /** Indices of tiles in the activation zone — the ONLY tiles to draw. */
  activeTiles: number[];
  draw: DrawContext;
  theme: ThemeLike;
  viewMode: GridViewMode;
  fitThumbnails: boolean;
  grayscale: boolean;
  showTileName: boolean;
  showResolution: boolean;
  showExtension: boolean;
  showExtensionLabel: boolean;
  showItemCount: boolean;
}

function fillPlaceholder(
  ctx: CanvasRenderingContext2D,
  item: CanvasRenderItem,
  theme: ThemeLike,
  fit: 'cover' | 'contain',
  x: number, y: number, w: number, h: number,
  alpha = 1,
): void {
  const hasContainShape = fit === 'contain' && !!item.aspectRatio;
  const previousAlpha = ctx.globalAlpha;

  if (hasContainShape) {
    ctx.globalAlpha = previousAlpha * alpha;
    const rect = getContainRect(item.aspectRatio ?? 1, x, y, w, h);
    ctx.fillStyle = theme.placeholderBg;
    ctx.beginPath();
    ctx.roundRect(rect.x, rect.y, rect.w, rect.h, theme.borderRadius);
    ctx.fill();
    if (item.dominantColor) {
      ctx.fillStyle = item.dominantColor;
      ctx.beginPath();
      ctx.roundRect(rect.x, rect.y, rect.w, rect.h, theme.borderRadius);
      ctx.fill();
    }
    ctx.globalAlpha = previousAlpha;
    return;
  }

  ctx.globalAlpha = previousAlpha * alpha;
  ctx.fillStyle = item.dominantColor ?? theme.placeholderBg;
  ctx.fillRect(x, y, w, h);
  ctx.globalAlpha = previousAlpha;
}

export function drawCanvasBaseLayer({
  ctx,
  positions,
  items,
  atlasGet,
  revealProgress,
  activeTiles,
  draw,
  theme,
  viewMode,
  fitThumbnails,
  grayscale,
  showTileName,
  showResolution,
  showExtension,
  showExtensionLabel,
  showItemCount,
}: BaseLayerArgs): boolean {
  const { scrollTop, viewportHeight: cssH, textHeight: th, borderRadius: br } = draw;
  // Grid default = contain. fitThumbnails flips grid to cover (fill/crop).
  // Waterfall/justified = always cover.
  const effectiveFit = viewMode === 'grid'
    ? (fitThumbnails ? 'cover' as const : 'contain' as const)
    : 'cover' as const;
  const shouldContain = (item: CanvasRenderItem) => effectiveFit === 'contain'
    || item.mime.startsWith('video/')
    || item.mime.startsWith('audio/');
  let hasActiveReveal = false;

  // ── Pass 1: Images with reveal animation ──
  for (const i of activeTiles) {
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item) continue;

    const drawY = pos.y - scrollTop;
    const imageHeight = pos.h - th;
    if (drawY + pos.h < 0 || drawY > cssH) continue;

    const entry = atlasGet(item.displayFileHash);
    const isAudio = item.mime.startsWith('audio/');
    const useContain = shouldContain(item);
    const drawThumb = useContain ? drawImageContain : drawImageCover;
    const drawLoadedThumb = (alpha = 1) => {
      const previousAlpha = ctx.globalAlpha;
      const previousFilter = ctx.filter;
      if (isAudio && item.aspectRatio) {
        const rect = getContainRect(item.aspectRatio, pos.x, drawY, pos.w, imageHeight);
        ctx.globalAlpha = previousAlpha * alpha;
        ctx.fillStyle = theme.isLight ? 'rgba(44, 47, 50, 0.05)' : 'rgba(247, 248, 248, 0.05)';
        ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
        const gradient = theme.isLight
          ? ctx.createLinearGradient(rect.x, rect.y, rect.x, rect.y + rect.h)
          : ctx.createRadialGradient(rect.x + rect.w / 2, rect.y, 0, rect.x + rect.w / 2, rect.y, Math.max(rect.w, rect.h));
        gradient.addColorStop(0, theme.isLight ? 'rgba(44, 47, 50, 0.02)' : 'rgba(247, 248, 248, 0.05)');
        gradient.addColorStop(1, theme.isLight ? 'rgba(44, 47, 50, 0.05)' : 'rgba(247, 248, 248, 0.03)');
        ctx.fillStyle = gradient;
        ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
        ctx.globalAlpha = previousAlpha * alpha * 0.5;
        ctx.filter = theme.isLight ? 'grayscale(1)' : 'grayscale(1) brightness(1.5)';
        drawThumb(ctx, entry!.thumb!, pos.x, drawY, pos.w, imageHeight);
        ctx.filter = previousFilter;
      } else {
        ctx.globalAlpha = previousAlpha * alpha;
        if (grayscale) ctx.filter = 'grayscale(1)';
        drawThumb(ctx, entry!.thumb!, pos.x, drawY, pos.w, imageHeight);
        ctx.filter = previousFilter;
      }
      ctx.globalAlpha = previousAlpha;
    };

    ctx.save();

    // Clip to tile area with rounded corners
    if (useContain && item.aspectRatio) {
      const rect = getContainRect(item.aspectRatio, pos.x, drawY, pos.w, imageHeight);
      ctx.beginPath();
      ctx.roundRect(rect.x, rect.y, rect.w, rect.h, br);
      ctx.clip();
    } else {
      ctx.beginPath();
      ctx.roundRect(pos.x, drawY, pos.w, imageHeight, br);
      ctx.clip();
    }

    if (grayscale) ctx.filter = 'grayscale(1)';

    if (item.mime.startsWith('font/')) {
      fillPlaceholder(ctx, item, theme, effectiveFit, pos.x, drawY, pos.w, imageHeight);
      drawFontThumbnail(ctx, pos.x, drawY, pos.w, imageHeight, theme.placeholderBg);
    // Two-phase reveal: image fades during the first half, then the
    // placeholder fades during the second half. Entity visibility, not
    // bitmap residency, owns the animation timeline.
    } else if (entry?.thumb) {
      const progress = revealProgress(item.hash);
      const imageProgress = Math.min(1, progress * 2);
      const placeholderAlpha = progress < 0.5 ? 1 : Math.max(0, 2 - progress * 2);

      if (imageProgress < 1 || placeholderAlpha > 0) {
        hasActiveReveal = true;
      }

      if (placeholderAlpha > 0) {
        fillPlaceholder(ctx, item, theme, effectiveFit, pos.x, drawY, pos.w, imageHeight, placeholderAlpha);
      }

      if (imageProgress < 1) {
        drawLoadedThumb(imageProgress);
      } else {
        drawLoadedThumb();
      }
    } else {
      if (entry?.state === 'error') {
        fillPlaceholder(ctx, item, theme, effectiveFit, pos.x, drawY, pos.w, imageHeight);
        drawBrokenThumbnail(ctx, pos.x, drawY, pos.w, imageHeight, theme.placeholderBg);
      } else {
        fillPlaceholder(ctx, item, theme, effectiveFit, pos.x, drawY, pos.w, imageHeight);
      }
    }

    ctx.restore();
  }

  // ── Pass 2: Glass border ring ──
  ctx.strokeStyle = theme.glassBorder;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (const i of activeTiles) {
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item) continue;
    const drawY = pos.y - scrollTop;
    const imageHeight = pos.h - th;
    if (drawY + pos.h < 0 || drawY > cssH) continue;
    if (shouldContain(item) && item.aspectRatio) {
      const rect = getContainRect(item.aspectRatio, pos.x, drawY, pos.w, imageHeight);
      ctx.roundRect(rect.x + 0.5, rect.y + 0.5, rect.w - 1, rect.h - 1, br);
    } else {
      ctx.roundRect(pos.x + 0.5, drawY + 0.5, pos.w - 1, imageHeight - 1, br);
    }
  }
  ctx.stroke();

  // ── Pass 3: Badges (extension, duration) ──
  for (const i of activeTiles) {
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item) continue;
    const drawY = pos.y - scrollTop;
    if (drawY + pos.h < 0 || drawY > cssH) continue;

    const imgH = pos.h - th;
    let bx = pos.x;
    let by = drawY;
    let bw = pos.w;
    let bh = imgH;
    if (shouldContain(item) && item.aspectRatio) {
      const rect = getContainRect(item.aspectRatio, pos.x, drawY, pos.w, imgH);
      bx = rect.x;
      by = rect.y;
      bw = rect.w;
      bh = rect.h;
    }

    const formatLabel = formatLabelForMime(item.mime);
    const isVideo = item.mime.startsWith('video/');
    const isAudio = item.mime.startsWith('audio/');
    const isAnimated = item.mime === 'image/gif' && (item.numFrames ?? 0) > 1;
    const showBadge = showExtensionLabel && formatLabel && item.kind !== 'collection';

    // Item-kind / extension badge — top-left
    if (showExtensionLabel && item.kind === 'collection') {
      drawGroupBadge(ctx, bx + 5, by + 5);
    } else if (showBadge) {
      drawBadge(ctx, formatLabel, bx + 5, by + 5, 'left', 'inspector');
    }

    // Duration badge — top-right (video/animated only)
    if ((isVideo || isAudio || isAnimated) && typeof item.durationMs === 'number' && item.durationMs > 0) {
      drawBadge(
        ctx,
        formatDuration(item.durationMs),
        bx + bw - GRID_BADGE_INSET,
        by + GRID_BADGE_INSET,
        'right',
      );
    }

    if (showItemCount && item.kind === 'collection') {
      drawBadge(
        ctx,
        String(item.mediaCount),
        bx + 5,
        by + bh - BADGE_H - 5,
        'left',
        'inspector',
      );
    }

  }

  // ── Pass 4: Name and resolution text ──
  if ((showTileName || showResolution) && th > 0) {
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';

    if (showTileName) {
      ctx.font = NAME_FONT;
      ctx.fillStyle = theme.textPrimary;
      for (const i of activeTiles) {
        const pos = positions[i];
        const item = items[i];
        if (!pos || !item) continue;
        const drawY = pos.y - scrollTop;
        const imageHeight = pos.h - th;
        if (drawY + pos.h < 0 || drawY > cssH) continue;
        const textX = pos.x + pos.w / 2;
        const nameY = drawY + imageHeight + NAME_BASELINE;
        const textMaxW = pos.w - 8;
        const ext = mimeToExt(item.mime);
        const nameStr = (item.name || 'Untitled') + (showExtension && ext ? `.${ext.toUpperCase()}` : '');
        ctx.fillText(truncateText(ctx, nameStr, textMaxW), textX, nameY);
      }
    }

    if (showResolution) {
      ctx.font = INFO_FONT;
      ctx.fillStyle = theme.textTertiary;
      const resOffset = showTileName ? 20 : 0;
      for (const i of activeTiles) {
        const pos = positions[i];
        const item = items[i];
        if (!pos || !item || !item.width || !item.height) continue;
        const drawY = pos.y - scrollTop;
        const imageHeight = pos.h - th;
        if (drawY + pos.h < 0 || drawY > cssH) continue;
        const infoY = drawY + imageHeight + INFO_BASELINE + resOffset;
        ctx.fillText(`${item.width} × ${item.height}`, pos.x + pos.w / 2, infoY);
      }
    }
  }

  return hasActiveReveal;
}

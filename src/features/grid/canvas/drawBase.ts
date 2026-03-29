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
  BADGE_FONT,
  BADGE_H,
  INFO_FONT,
  NAME_FONT,
  drawBadge,
  drawImageContain,
  drawImageCover,
  getContainRect,
  isHiddenBadgeType,
  mimeToExt,
  truncateText,
  formatDuration,
} from './primitives';

const GLASS_BORDER_COLOR = 'rgba(255, 255, 255, 0.2)';

interface ThemeLike {
  placeholderBg: string;
  borderRadius: number;
  textPrimary: string;
  textTertiary: string;
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
  atlasGet: (hash: string) => ThumbnailPipelineEntry | null;
  /** Per-hash reveal progress (0–1). Missing = fully revealed. */
  revealProgressByHash: Map<string, number>;
  /** Indices of tiles in the activation zone — the ONLY tiles to draw. */
  activeTiles: number[];
  draw: DrawContext;
  theme: ThemeLike;
  viewMode: GridViewMode;
  fitThumbnails: boolean;
  showTileName: boolean;
  showResolution: boolean;
  showExtension: boolean;
  showExtensionLabel: boolean;
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
    ctx.fillStyle = theme.placeholderBg;
    ctx.fillRect(x, y, w, h);
    if (item.dominantColor) {
      const rect = getContainRect(item.aspectRatio ?? 1, x, y, w, h);
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
  revealProgressByHash,
  activeTiles,
  draw,
  theme,
  viewMode,
  fitThumbnails,
  showTileName,
  showResolution,
  showExtension,
  showExtensionLabel,
}: BaseLayerArgs): boolean {
  const { scrollTop, viewportHeight: cssH, textHeight: th, borderRadius: br } = draw;
  // Grid default = contain. fitThumbnails flips grid to cover (fill/crop).
  // Waterfall/justified = always cover.
  const effectiveFit = viewMode === 'grid'
    ? (fitThumbnails ? 'cover' as const : 'contain' as const)
    : 'cover' as const;
  let hasActiveReveal = false;

  // ── Pass 1: Images with reveal animation ──
  for (const i of activeTiles) {
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item) continue;

    const drawY = pos.y - scrollTop;
    const imageHeight = pos.h - th;
    if (drawY + pos.h < 0 || drawY > cssH) continue;

    const entry = atlasGet(item.thumbnailHash);
    const isVideo = item.mime.startsWith('video/');
    const useContain = effectiveFit === 'contain' || isVideo;
    const drawThumb = useContain ? drawImageContain : drawImageCover;

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

    const hasBitmap = entry?.thumb != null;
    const progress = revealProgressByHash.get(item.thumbnailHash) ?? 1;

    if (hasBitmap && progress > 0) {
      if (progress < 1) {
        hasActiveReveal = true;
        fillPlaceholder(ctx, item, theme, effectiveFit, pos.x, drawY, pos.w, imageHeight);
        ctx.globalAlpha = progress;
        drawThumb(ctx, entry!.thumb!, pos.x, drawY, pos.w, imageHeight);
        ctx.globalAlpha = 1;
      } else {
        drawThumb(ctx, entry!.thumb!, pos.x, drawY, pos.w, imageHeight);
      }
    } else {
      fillPlaceholder(ctx, item, theme, effectiveFit, pos.x, drawY, pos.w, imageHeight);
    }

    ctx.restore();
  }

  // ── Pass 2: Glass border ring ──
  ctx.strokeStyle = GLASS_BORDER_COLOR;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (const i of activeTiles) {
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item) continue;
    const drawY = pos.y - scrollTop;
    const imageHeight = pos.h - th;
    if (drawY + pos.h < 0 || drawY > cssH) continue;
    if (effectiveFit === 'contain' && item.aspectRatio) {
      const rect = getContainRect(item.aspectRatio, pos.x, drawY, pos.w, imageHeight);
      ctx.roundRect(rect.x + 0.5, rect.y + 0.5, rect.w - 1, rect.h - 1, br);
    } else {
      ctx.roundRect(pos.x + 0.5, drawY + 0.5, pos.w - 1, imageHeight - 1, br);
    }
  }
  ctx.stroke();

  // ── Pass 3: Badges (extension, duration, collection count) ──
  const isContain = effectiveFit === 'contain';
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
    if (isContain && item.kind !== 'collection' && item.aspectRatio) {
      const rect = getContainRect(item.aspectRatio, pos.x, drawY, pos.w, imgH);
      bx = rect.x;
      by = rect.y;
      bw = rect.w;
    }

    const ext = mimeToExt(item.mime);
    const isVideo = item.mime.startsWith('video/');
    const isAnimated = item.mime === 'image/gif' && (item.numFrames ?? 0) > 1;
    const isCollection = item.kind === 'collection';
    const showBadge = !isCollection && showExtensionLabel && ext && !isHiddenBadgeType(ext);

    // Extension badge — top-left
    if (showBadge) {
      drawBadge(ctx, ext.toUpperCase(), bx + 5, by + 5);
    }

    // Duration badge — top-right (video/animated only)
    if ((isVideo || isAnimated) && typeof item.durationMs === 'number' && item.durationMs > 0) {
      const durText = formatDuration(item.durationMs);
      ctx.font = BADGE_FONT;
      const durW = ctx.measureText(durText).width + 8;
      drawBadge(ctx, durText, bx + bw - durW - 5, by + 5);
    }

    // Collection count badge — bottom-left
    if (isCollection) {
      const itemCount = Math.max(0, item.memberCount ?? 0);
      drawBadge(ctx, `${itemCount.toLocaleString()} items`, bx + 5, by + imgH - BADGE_H - 5);
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
        const nameY = drawY + imageHeight + 14;
        const textMaxW = pos.w - 8;
        const ext = mimeToExt(item.mime);
        const nameStr = (item.name || 'Untitled') + (showExtension && ext ? `.${ext}` : '');
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
        ctx.fillText(`${item.width} × ${item.height}`, pos.x + pos.w / 2, drawY + imageHeight + 14 + resOffset);
      }
    }
  }

  return hasActiveReveal;
}

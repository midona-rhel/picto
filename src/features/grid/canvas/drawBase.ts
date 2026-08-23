import type { LayoutItem, GridViewMode } from '../layout/types';
import type { CanvasRenderItem } from './renderItemAdapter';
import type { ThumbnailPipelineEntry } from './thumbnailPipeline';
import {
  BADGE_FONT,
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
interface ThemeLike {
  placeholderBg: string;
  borderRadius: number;
  textPrimary: string;
  textTertiary: string;
  glassBorder: string;
  tileBoundary: string;
}

export interface DrawContext {
  scrollTop: number;
  textHeight: number;
  borderRadius: number;
}

export interface BaseLayerArgs {
  ctx: CanvasRenderingContext2D;
  positions: LayoutItem[];
  items: CanvasRenderItem[];
  atlasGet: (hash: string) => ThumbnailPipelineEntry | null;
  revealProgress: (entityHash: string) => number;
  visibleTiles: number[];
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
  revealProgress,
  visibleTiles,
  draw,
  theme,
  viewMode,
  fitThumbnails,
  showTileName,
  showResolution,
  showExtension,
  showExtensionLabel,
}: BaseLayerArgs): boolean {
  const { scrollTop, textHeight: th, borderRadius: br } = draw;
  const effectiveFit = viewMode === 'grid'
    ? (fitThumbnails ? 'cover' as const : 'contain' as const)
    : 'cover' as const;
  let hasActiveReveal = false;

  for (const i of visibleTiles) {
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item) continue;

    const drawY = pos.y - scrollTop;
    const imageHeight = pos.h - th;

    const entry = atlasGet(item.thumbnailHash);
    const isVideo = item.mime.startsWith('video/');
    const useContain = effectiveFit === 'contain' || isVideo;
    const drawThumb = useContain ? drawImageContain : drawImageCover;

    ctx.save();

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

    if (entry?.thumb) {
      const progress = revealProgress(item.hash);
      const imageProgress = Math.min(1, progress * 2);
      const placeholderAlpha = imageProgress < 1
        ? 1
        : Math.max(0, 2 - (progress * 2));

      if (imageProgress < 1 || placeholderAlpha > 0) {
        hasActiveReveal = true;
      }

      if (placeholderAlpha > 0) {
        fillPlaceholder(ctx, item, theme, effectiveFit, pos.x, drawY, pos.w, imageHeight, placeholderAlpha);
      }

      if (imageProgress < 1) {
        ctx.globalAlpha = imageProgress;
        drawThumb(ctx, entry.thumb, pos.x, drawY, pos.w, imageHeight);
        ctx.globalAlpha = 1;
      } else {
        drawThumb(ctx, entry.thumb, pos.x, drawY, pos.w, imageHeight);
      }
    } else {
      fillPlaceholder(ctx, item, theme, effectiveFit, pos.x, drawY, pos.w, imageHeight);
    }

    ctx.restore();
  }

  ctx.strokeStyle = theme.glassBorder;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (const i of visibleTiles) {
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item) continue;
    const drawY = pos.y - scrollTop;
    const imageHeight = pos.h - th;
    if (effectiveFit === 'contain' && item.aspectRatio) {
      const rect = getContainRect(item.aspectRatio, pos.x, drawY, pos.w, imageHeight);
      ctx.roundRect(rect.x + 0.5, rect.y + 0.5, rect.w - 1, rect.h - 1, br);
    } else {
      ctx.roundRect(pos.x + 0.5, drawY + 0.5, pos.w - 1, imageHeight - 1, br);
    }
  }
  ctx.stroke();

  if (effectiveFit === 'contain') {
    ctx.strokeStyle = theme.tileBoundary;
    ctx.lineWidth = 2;
    ctx.beginPath();
    for (const i of visibleTiles) {
      const pos = positions[i];
      const item = items[i];
      if (!pos || !item || !item.aspectRatio) continue;
      const drawY = pos.y - scrollTop;
      const imageHeight = pos.h - th;
      const rect = getContainRect(item.aspectRatio, pos.x, drawY, pos.w, imageHeight);
      if (rect.w < pos.w - 2 || rect.h < imageHeight - 2) {
        ctx.roundRect(pos.x + 0.5, drawY + 0.5, pos.w - 1, imageHeight - 1, br);
      }
    }
    ctx.stroke();
  }

  const isContain = effectiveFit === 'contain';
  for (const i of visibleTiles) {
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item) continue;
    const drawY = pos.y - scrollTop;

    const imgH = pos.h - th;
    let bx = pos.x;
    let by = drawY;
    let bw = pos.w;
    if (isContain && item.aspectRatio) {
      const rect = getContainRect(item.aspectRatio, pos.x, drawY, pos.w, imgH);
      bx = rect.x;
      by = rect.y;
      bw = rect.w;
    }

    const ext = mimeToExt(item.mime);
    const isVideo = item.mime.startsWith('video/');
    const isAnimated = item.mime === 'image/gif' && (item.numFrames ?? 0) > 1;
    const showBadge = showExtensionLabel && ext && !isHiddenBadgeType(ext);

    if (showBadge) {
      drawBadge(ctx, ext.toUpperCase(), bx + 5, by + 5);
    }

    if ((isVideo || isAnimated) && typeof item.durationMs === 'number' && item.durationMs > 0) {
      const durText = formatDuration(item.durationMs);
      ctx.font = BADGE_FONT;
      const durW = ctx.measureText(durText).width + 8;
      drawBadge(ctx, durText, bx + bw - durW - 5, by + 5);
    }
  }

  if ((showTileName || showResolution) && th > 0) {
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';

    if (showTileName) {
      ctx.font = NAME_FONT;
      ctx.fillStyle = theme.textPrimary;
      for (const i of visibleTiles) {
        const pos = positions[i];
        const item = items[i];
        if (!pos || !item) continue;
        const drawY = pos.y - scrollTop;
        const imageHeight = pos.h - th;
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
      for (const i of visibleTiles) {
        const pos = positions[i];
        const item = items[i];
        if (!pos || !item || !item.width || !item.height) continue;
        const drawY = pos.y - scrollTop;
        const imageHeight = pos.h - th;
        ctx.fillText(`${item.width} × ${item.height}`, pos.x + pos.w / 2, drawY + imageHeight + 14 + resOffset);
      }
    }
  }

  return hasActiveReveal;
}

import type { LayoutItem } from '../layout/types';
import type { GridViewMode } from '../layout/types';
import type { CanvasRenderItem } from './renderItemAdapter';
import type { ThumbnailPipelineEntry } from './thumbnailPipeline';
import {
  BADGE_FONT,
  BADGE_H,
  NAME_FONT,
  RATING_FONT,
  drawBadge,
  drawImageContain,
  drawImageCover,
  getContainRect,
  isHiddenBadgeType,
  mimeToExt,
  truncateText,
  formatDuration,
} from './primitives';
import { THUMBNAIL_PIPELINE_REVEAL_MS } from './thumbnailPipelinePolicy';

const GLASS_BORDER_COLOR = 'rgba(255, 255, 255, 0.2)';

interface ThemeLike {
  placeholderBg: string;
  borderRadius: number;
}

export interface VisibleWindow {
  startIdx: number;
  endIdx: number;
  visibleIndices: number[] | null;
  visibleIterEnd: number;
  scrollTop: number;
  cssH: number;
  th: number;
  br: number;
}

export interface BaseLayerArgs {
  ctx: CanvasRenderingContext2D;
  positions: LayoutItem[];
  items: CanvasRenderItem[];
  atlasGet: (hash: string) => ThumbnailPipelineEntry | null;
  atlasEnsure: (hash: string, args?: { y?: number; drawWidth?: number; drawHeight?: number }) => void;
  now: number;
  visible: VisibleWindow;
  theme: ThemeLike;
  viewMode: GridViewMode;
  showTileName: boolean;
  showExtension: boolean;
}

function fillPlaceholder(
  ctx: CanvasRenderingContext2D,
  item: CanvasRenderItem,
  theme: ThemeLike,
  fit: 'cover' | 'contain',
  x: number,
  y: number,
  w: number,
  h: number,
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
  atlasEnsure,
  now,
  visible,
  theme,
  viewMode,
  showTileName,
  showExtension,
}: BaseLayerArgs): boolean {
  const { startIdx, endIdx, visibleIndices, visibleIterEnd, scrollTop, cssH, th, br } = visible;
  const effectiveFit = viewMode === 'grid' ? 'contain' as const : 'cover' as const;
  let hasActiveReveal = false;

  for (let n = 0; n < visibleIterEnd; n += 1) {
    const i = visibleIndices ? visibleIndices[n] : startIdx + n;
    if (i >= endIdx) break;
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item) continue;

    const drawY = pos.y - scrollTop;
    const imageHeight = pos.h - th;
    if (drawY + pos.h < 0 || drawY > cssH) continue;

    atlasEnsure(item.thumbnailHash, {
      y: pos.y + pos.h / 2,
      drawWidth: pos.w,
      drawHeight: imageHeight,
    });

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

    if (entry?.thumb && entry.state === 'shown') {
      const revealElapsedMs = entry.animateIn
        ? Math.max(0, now - entry.revealStartedAt)
        : THUMBNAIL_PIPELINE_REVEAL_MS * 2;
      const imageProgress = Math.min(1, revealElapsedMs / THUMBNAIL_PIPELINE_REVEAL_MS);
      const placeholderFadeElapsedMs = Math.max(0, revealElapsedMs - THUMBNAIL_PIPELINE_REVEAL_MS);
      const placeholderAlpha = imageProgress < 1
        ? 1
        : Math.max(0, 1 - (placeholderFadeElapsedMs / THUMBNAIL_PIPELINE_REVEAL_MS));

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

  ctx.strokeStyle = GLASS_BORDER_COLOR;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let n = 0; n < visibleIterEnd; n += 1) {
    const i = visibleIndices ? visibleIndices[n] : startIdx + n;
    if (i >= endIdx) break;
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

  ctx.font = BADGE_FONT;
  for (let n = 0; n < visibleIterEnd; n += 1) {
    const i = visibleIndices ? visibleIndices[n] : startIdx + n;
    if (i >= endIdx) break;
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item) continue;
    const drawY = pos.y - scrollTop;
    if (drawY + pos.h < 0 || drawY > cssH) continue;

    const imageHeight = pos.h - th;
    let badgeX = pos.x + pos.w - 4;
    const badgeY = drawY + 4;

    if (item.durationMs != null) {
      const duration = formatDuration(item.durationMs);
      badgeX -= drawBadge(ctx, duration, badgeX, badgeY, 'right') + 4;
    }

    if (item.kind === 'collection' && item.memberCount != null) {
      drawBadge(ctx, String(item.memberCount), badgeX, badgeY, 'right');
    }

    if (showExtension) {
      const ext = mimeToExt(item.mime);
      if (ext && !isHiddenBadgeType(ext)) {
        drawBadge(ctx, ext, pos.x + pos.w - 4, drawY + imageHeight - BADGE_H - 4, 'right');
      }
    }
  }

  ctx.font = RATING_FONT;
  ctx.fillStyle = '#ffd54f';
  ctx.textBaseline = 'top';
  for (let n = 0; n < visibleIterEnd; n += 1) {
    const i = visibleIndices ? visibleIndices[n] : startIdx + n;
    if (i >= endIdx) break;
    const pos = positions[i];
    const item = items[i];
    if (!pos || !item || item.rating == null || item.rating <= 0) continue;
    const drawY = pos.y - scrollTop;
    if (drawY + pos.h < 0 || drawY > cssH) continue;
    ctx.fillText('★'.repeat(item.rating), pos.x + 5, drawY + 5);
  }

  if (showTileName && th > 0) {
    ctx.font = NAME_FONT;
    ctx.fillStyle = 'rgba(255, 255, 255, 0.85)';
    ctx.textBaseline = 'top';
    for (let n = 0; n < visibleIterEnd; n += 1) {
      const i = visibleIndices ? visibleIndices[n] : startIdx + n;
      if (i >= endIdx) break;
      const pos = positions[i];
      const item = items[i];
      if (!pos || !item || !item.name) continue;
      const drawY = pos.y - scrollTop;
      if (drawY + pos.h < 0 || drawY > cssH) continue;
      const nameY = drawY + (pos.h - th) + 3;
      const displayName = truncateText(ctx, item.name, pos.w - 4);
      ctx.fillText(displayName, pos.x + 2, nameY);
    }
  }

  return hasActiveReveal;
}

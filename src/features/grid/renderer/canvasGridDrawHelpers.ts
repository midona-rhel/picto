import { formatDuration } from '../../../shared/lib/formatters';
import { TEXT_NAME_ROW_H } from '../gridLayout';
import type { LayoutItem } from '../layoutMath';
import type { ThumbnailPipelineEntry } from '../../../shared/lib/canvas/thumbnailPipeline';
import { isVideoMime, type MasonryImageItem } from '../shared';
import type { GridViewMode } from '../runtime';
import {
  BADGE_FONT,
  BADGE_H,
  INFO_FONT,
  NAME_FONT,
  drawBadge,
  getContainRect,
  drawImageContain,
  drawImageCover,
  isHiddenBadgeType,
  mimeToExt,
  truncateText,
} from './canvasGridPrimitives';
import {
  THUMBNAIL_PIPELINE_SOURCE_EDGE,
  THUMBNAIL_PIPELINE_FULL_QUALITY_THRESHOLD,
  THUMBNAIL_PIPELINE_REVEAL_MS,
} from '../../../shared/lib/canvas/thumbnailPipelinePolicy';
import { getNavigationImageAdjustment } from '../../../state/navigationImageAdjustmentsStore';

const FULL_QUALITY_THRESHOLD_PX = Math.round(
  THUMBNAIL_PIPELINE_SOURCE_EDGE * THUMBNAIL_PIPELINE_FULL_QUALITY_THRESHOLD,
);

interface ThemeLike {
  primaryColor: string;
  textPrimary: string;
  textTertiary: string;
  placeholderBg: string;
  borderRadius: number;
  innerBorder: string;
}

interface VisibleWindow {
  startIdx: number;
  visibleIndices: number[] | null;
  visibleIterEnd: number;
  scrollTop: number;
  cssH: number;
  th: number;
  br: number;
}

interface BaseLayerArgs {
  ctx: CanvasRenderingContext2D;
  positions: LayoutItem[];
  imgs: MasonryImageItem[];
  atlasGet: (hash: string) => ThumbnailPipelineEntry | null;
  atlasEnsure: (hash: string, args?: {
    y?: number;
    drawWidth?: number;
    drawHeight?: number;
    mime?: string;
    sourceWidth?: number | null;
    sourceHeight?: number | null;
  }) => void;
  now: number;
  theme: ThemeLike;
  visible: VisibleWindow;
  thumbnailFitMode: 'cover' | 'contain';
  renamingHash: string | null;
  showTileName: boolean;
  showResolution: boolean;
  showExtension: boolean;
  showExtensionLabel: boolean;
  videoScrubIdx: number | null;
  viewMode: GridViewMode;
}

function fillPlaceholder(
  ctx: CanvasRenderingContext2D,
  image: MasonryImageItem,
  theme: ThemeLike,
  fit: 'cover' | 'contain',
  x: number,
  y: number,
  w: number,
  h: number,
  alpha = 1,
): void {
  const hasContainShape = fit === 'contain' && !!image.aspectRatio;
  const previousAlpha = ctx.globalAlpha;

  if (hasContainShape) {
    ctx.globalAlpha = previousAlpha * alpha;
    ctx.fillStyle = theme.placeholderBg;
    ctx.fillRect(x, y, w, h);
    if (image.dominant_color_hex) {
      const rect = getContainRect(image.aspectRatio, x, y, w, h);
      ctx.fillStyle = image.dominant_color_hex;
      ctx.beginPath();
      ctx.roundRect(rect.x, rect.y, rect.w, rect.h, theme.borderRadius);
      ctx.fill();
    }
    ctx.globalAlpha = previousAlpha;
    return;
  }

  ctx.globalAlpha = previousAlpha * alpha;
  ctx.fillStyle = image.dominant_color_hex || theme.placeholderBg;
  ctx.fillRect(x, y, w, h);
  ctx.globalAlpha = previousAlpha;
}

export function drawCanvasBaseLayer({
  ctx,
  positions,
  imgs,
  atlasGet,
  atlasEnsure,
  now,
  theme,
  visible,
  thumbnailFitMode,
  renamingHash,
  showTileName,
  showResolution,
  showExtension,
  showExtensionLabel,
  videoScrubIdx,
  viewMode,
}: BaseLayerArgs): boolean {
  const { startIdx, visibleIndices, visibleIterEnd, scrollTop, cssH, th, br } = visible;
  // thumbnailFitMode only applies in grid view; waterfall/justified always use cover
  const effectiveFit = viewMode === 'grid' ? thumbnailFitMode : 'cover' as const;
  let hasActiveReveal = false;

  for (let n = 0; n < visibleIterEnd; n++) {
    const i = visibleIndices ? visibleIndices[n] : startIdx + n;
    const pos = positions[i];
    const image = imgs[i];
    if (!image) continue;
    const drawY = pos.y - scrollTop;
    const imageHeight = pos.h - th;
    if (drawY + pos.h < 0 || drawY > cssH) continue;

    // Short-circuit: skip ensure() for warm cache entries. Only call ensure()
    // when the entry is missing, not yet shown, or needs a quality upgrade.
    // This eliminates ~90% of per-frame buildRequest() overhead.
    const entry = atlasGet(image.hash);
    const dprLongEdge = Math.max(
      pos.w * (window.devicePixelRatio || 1),
      imageHeight * (window.devicePixelRatio || 1),
    );
    if (
      !entry
      || entry.state !== 'shown'
      || (entry.sourceKind === 'thumbnail' && dprLongEdge > FULL_QUALITY_THRESHOLD_PX)
      || (dprLongEdge > entry.loadedLongEdge * 1.15)
    ) {
      atlasEnsure(image.hash, {
        y: pos.y + pos.h / 2,
        drawWidth: pos.w * (window.devicePixelRatio || 1),
        drawHeight: imageHeight * (window.devicePixelRatio || 1),
        mime: image.mime,
        sourceWidth: image.width,
        sourceHeight: image.height,
      });
    }

    ctx.save();
    const adjustment = getNavigationImageAdjustment(image.hash);
    if (adjustment.grayscale) {
      ctx.filter = 'grayscale(1)';
    }

    // Clip to the actual image area (rounded corners on the image itself, not just the tile)
    const isVideo = image.mime.startsWith('video/');
    const useContain = effectiveFit === 'contain' || isVideo;
    if (useContain && image.aspectRatio) {
      const cr = getContainRect(image.aspectRatio, pos.x, drawY, pos.w, imageHeight);
      ctx.beginPath();
      ctx.roundRect(cr.x, cr.y, cr.w, cr.h, br);
      ctx.clip();
    } else {
      ctx.beginPath();
      ctx.roundRect(pos.x, drawY, pos.w, imageHeight, br);
      ctx.clip();
    }

    const drawThumb = useContain ? drawImageContain : drawImageCover;
    if (entry?.thumb) {
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
        fillPlaceholder(ctx, image, theme, effectiveFit, pos.x, drawY, pos.w, imageHeight, placeholderAlpha);
      }

      if (imageProgress < 1) {
        ctx.globalAlpha = imageProgress;
        drawThumb(ctx, entry.thumb, pos.x, drawY, pos.w, imageHeight);
        ctx.globalAlpha = 1;
      } else {
        drawThumb(ctx, entry.thumb, pos.x, drawY, pos.w, imageHeight);
      }
    } else {
      fillPlaceholder(ctx, image, theme, effectiveFit, pos.x, drawY, pos.w, imageHeight);
    }

    ctx.restore();
  }

  // Glass-like inner border ring on images
  ctx.strokeStyle = 'rgba(255, 255, 255, 0.2)';
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let n = 0; n < visibleIterEnd; n++) {
    const i = visibleIndices ? visibleIndices[n] : startIdx + n;
    const pos = positions[i];
    const image = imgs[i];
    const drawY = pos.y - scrollTop;
    const imageHeight = pos.h - th;
    if (drawY + pos.h < 0 || drawY > cssH) continue;
    if (effectiveFit === 'contain' && image?.aspectRatio) {
      const { x: ix, y: iy, w: iw, h: ih } = getContainRect(image.aspectRatio, pos.x, drawY, pos.w, imageHeight);
      ctx.roundRect(ix + 0.5, iy + 0.5, iw - 1, ih - 1, br);
    } else {
      ctx.roundRect(pos.x + 0.5, drawY + 0.5, pos.w - 1, imageHeight - 1, br);
    }
  }
  ctx.stroke();

  const isContain = effectiveFit === 'contain';
  for (let n = 0; n < visibleIterEnd; n++) {
    const i = visibleIndices ? visibleIndices[n] : startIdx + n;
    const pos = positions[i];
    const image = imgs[i];
    if (!image) continue;
    const drawY = pos.y - scrollTop;
    if (drawY + pos.h < 0 || drawY > cssH) continue;

    const imgH = pos.h - th;
    let bx = pos.x;
    let by = drawY;
    let bw = pos.w;
    if (isContain && !image.is_collection && image.aspectRatio) {
      const rect = getContainRect(image.aspectRatio, pos.x, drawY, pos.w, imgH);
      bx = rect.x;
      by = rect.y;
      bw = rect.w;
    }

    const ext = mimeToExt(image.mime);
    const isVideo = image.mime.startsWith('video/');
    const isAnimated = image.mime === 'image/gif' && (image.num_frames ?? 0) > 1;
    const isCollection = image.is_collection === true;
    const showBadge = !isCollection && showExtensionLabel && ext && !isHiddenBadgeType(ext);

    if (showBadge) {
      drawBadge(ctx, ext.toUpperCase(), bx + 5, by + 5);
    }

    if ((isVideo || isAnimated) && typeof image.duration_ms === 'number' && image.duration_ms > 0 && videoScrubIdx !== i) {
      const durText = formatDuration(image.duration_ms);
      ctx.font = BADGE_FONT;
      const durW = ctx.measureText(durText).width + 8;
      drawBadge(ctx, durText, bx + bw - durW - 5, by + 5);
    }

    if (isCollection) {
      const itemCount = Math.max(0, image.collection_item_count ?? 0);
      drawBadge(ctx, `${itemCount.toLocaleString()} items`, bx + 5, by + imgH - BADGE_H - 5);
    }
  }

  if ((showTileName || showResolution) && th > 0) {
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';

    if (showTileName) {
      ctx.font = NAME_FONT;
      ctx.fillStyle = theme.textPrimary;
      for (let n = 0; n < visibleIterEnd; n++) {
        const i = visibleIndices ? visibleIndices[n] : startIdx + n;
        const pos = positions[i];
        const image = imgs[i];
        if (!image || image.hash === renamingHash) continue;
        const drawY = pos.y - scrollTop;
        const imageHeight = pos.h - th;
        if (drawY + pos.h < 0 || drawY > cssH) continue;
        const textX = pos.x + pos.w / 2;
        const nameY = drawY + imageHeight + 14;
        const textMaxW = pos.w - 8;
        const ext = mimeToExt(image.mime);
        const nameStr = (image.name || 'Untitled') + (showExtension && ext ? `.${ext}` : '');
        ctx.fillText(truncateText(ctx, nameStr, textMaxW), textX, nameY);
      }
    }

    if (showResolution) {
      ctx.font = INFO_FONT;
      ctx.fillStyle = theme.textTertiary;
      const resOffset = showTileName ? TEXT_NAME_ROW_H : 0;
      for (let n = 0; n < visibleIterEnd; n++) {
        const i = visibleIndices ? visibleIndices[n] : startIdx + n;
        const pos = positions[i];
        const image = imgs[i];
        if (!image || !image.width || !image.height) continue;
        const drawY = pos.y - scrollTop;
        const imageHeight = pos.h - th;
        if (drawY + pos.h < 0 || drawY > cssH) continue;
        const textX = pos.x + pos.w / 2;
        ctx.fillText(`${image.width} × ${image.height}`, textX, drawY + imageHeight + 14 + resOffset);
      }
    }
  }

  return hasActiveReveal;
}

interface OverlayLayerArgs {
  ctx: CanvasRenderingContext2D;
  positions: LayoutItem[];
  imgs: MasonryImageItem[];
  theme: ThemeLike;
  visible: VisibleWindow;
  selected: Set<string>;
  hoveredIdx: number | null;
  marqueeRect: { left: number; top: number; width: number; height: number } | null;
  marqueeHitHashes: Set<string> | null;
  marqueeActive: boolean;
  isScrolling: boolean;
  zoomBtnSize: number;
  gap: number;
  reorderDrop:
    | {
        dropIndex: number | null;
        dropSide: 'left' | 'right' | null;
      }
    | null;
}

export function drawCanvasOverlayLayer({
  ctx,
  positions,
  imgs,
  theme,
  visible,
  selected,
  hoveredIdx,
  marqueeRect,
  marqueeHitHashes,
  marqueeActive,
  isScrolling,
  zoomBtnSize,
  gap,
  reorderDrop,
}: OverlayLayerArgs): void {
  const { startIdx, visibleIndices, visibleIterEnd, scrollTop, cssH, th, br } = visible;

  const hoveredImg = hoveredIdx != null ? imgs[hoveredIdx] : null;
  if (hoveredIdx != null && !isScrolling && hoveredImg && !hoveredImg.is_collection && !isVideoMime(hoveredImg.mime)) {
    const pos = positions[hoveredIdx];
    if (pos) {
      const drawY = pos.y - scrollTop;
      if (drawY + pos.h >= 0 && drawY <= cssH) {
        const imageHeight = pos.h - th;
        ctx.fillStyle = 'rgba(0, 0, 0, 0.4)';
        const bgW = zoomBtnSize + 4;
        const bgH = zoomBtnSize + 2;
        const zx = pos.x + pos.w - bgW;
        const zy = drawY + imageHeight - bgH;
        ctx.beginPath();
        ctx.roundRect(zx, zy, bgW, bgH, [10, 0, br, 0]);
        ctx.fill();
        ctx.strokeStyle = 'rgba(255, 255, 255, 0.7)';
        ctx.lineWidth = 1.5;
        const cx = zx + bgW / 2;
        const cy = zy + bgH / 2;
        ctx.beginPath();
        ctx.arc(cx, cy, 5, 0, Math.PI * 2);
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(cx + 3.5, cy + 3.5);
        ctx.lineTo(cx + 6, cy + 6);
        ctx.stroke();
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(cx, cy - 2.5);
        ctx.lineTo(cx, cy + 2.5);
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(cx - 2.5, cy);
        ctx.lineTo(cx + 2.5, cy);
        ctx.stroke();
      }
    }
  }

  ctx.strokeStyle = theme.primaryColor;
  ctx.lineWidth = 2;
  ctx.beginPath();
  let hasSelections = false;
  for (let n = 0; n < visibleIterEnd; n++) {
    const i = visibleIndices ? visibleIndices[n] : startIdx + n;
    const image = imgs[i];
    if (!image) continue;
    const isSelected = selected.has(image.hash) || (marqueeHitHashes?.has(image.hash) ?? false);
    if (!isSelected) continue;
    const pos = positions[i];
    const drawY = pos.y - scrollTop;
    if (drawY + pos.h < 0 || drawY > cssH) continue;
    const imgH = pos.h - th;
    ctx.roundRect(pos.x - 1, drawY - 1, pos.w + 2, imgH + 2, br);
    hasSelections = true;
  }
  if (hasSelections) ctx.stroke();

  if (marqueeRect && marqueeActive) {
    const mx = marqueeRect.left;
    const my = marqueeRect.top - scrollTop;
    ctx.fillStyle = 'rgba(51, 154, 240, 0.12)';
    ctx.fillRect(mx, my, marqueeRect.width, marqueeRect.height);
    ctx.strokeStyle = 'rgba(51, 154, 240, 0.5)';
    ctx.lineWidth = 1;
    ctx.strokeRect(mx + 0.5, my + 0.5, marqueeRect.width - 1, marqueeRect.height - 1);
  }

  if (reorderDrop?.dropIndex != null && reorderDrop.dropSide) {
    const pos = positions[reorderDrop.dropIndex];
    if (pos) {
      const indicatorX = reorderDrop.dropSide === 'left'
        ? pos.x - gap / 2
        : pos.x + pos.w + gap / 2;
      const drawY = pos.y - scrollTop;
      const drawH = pos.h;

      ctx.strokeStyle = '#228be6';
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(indicatorX, drawY);
      ctx.lineTo(indicatorX, drawY + drawH);
      ctx.stroke();

      ctx.fillStyle = '#228be6';
      ctx.beginPath();
      ctx.moveTo(indicatorX - 5, drawY);
      ctx.lineTo(indicatorX + 5, drawY);
      ctx.lineTo(indicatorX, drawY + 7);
      ctx.closePath();
      ctx.fill();
    }
  }
}

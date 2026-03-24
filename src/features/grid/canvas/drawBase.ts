/**
 * Canvas base layer drawing — thumbnails, placeholders, badges, text, borders.
 * Matches legacy canvasGridDrawHelpers.ts drawCanvasBaseLayer() rendering.
 */

import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { LayoutItem } from '../layout/types';
import { drawImageCover, drawBadge, truncateText, formatDuration, mimeToExt, NAME_FONT, BADGE_H } from './primitives';

const BORDER_RADIUS = 6;
const GLASS_BORDER_ALPHA = 0.2;
const PLACEHOLDER_COLOR = '#2a2a2e';

export interface BaseDrawParams {
  ctx: CanvasRenderingContext2D;
  items: CanonicalEntityGridItem[];
  positions: LayoutItem[];
  thumbnails: Map<string, ImageBitmap>;
  textHeight: number;
  visibleStart: number;
  visibleEnd: number;
  dpr: number;
  showName: boolean;
  showExtension: boolean;
}

export function drawBaseLayer(params: BaseDrawParams) {
  const { ctx, items, positions, thumbnails, textHeight, visibleStart, visibleEnd, dpr, showName, showExtension } = params;

  ctx.save();
  ctx.scale(dpr, dpr);

  for (let i = visibleStart; i < visibleEnd && i < items.length; i++) {
    const item = items[i];
    const pos = positions[i];
    if (!pos) continue;

    const imgH = pos.h - textHeight;
    const tx = pos.x;
    const ty = pos.y;

    // Clip to rounded rect
    ctx.save();
    ctx.beginPath();
    ctx.roundRect(tx, ty, pos.w, imgH, BORDER_RADIUS);
    ctx.clip();

    // Placeholder fill
    ctx.fillStyle = item.dominant_color_hex ?? PLACEHOLDER_COLOR;
    ctx.fillRect(tx, ty, pos.w, imgH);

    // Thumbnail
    const bitmap = thumbnails.get(item.entity_hash);
    if (bitmap) {
      drawImageCover(ctx, bitmap, tx, ty, pos.w, imgH);
    }

    // Glass inner border
    ctx.strokeStyle = `rgba(255, 255, 255, ${GLASS_BORDER_ALPHA})`;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.roundRect(tx + 0.5, ty + 0.5, pos.w - 1, imgH - 1, BORDER_RADIUS - 0.5);
    ctx.stroke();

    ctx.restore(); // Unclip

    // Badges (top-right of image area)
    let badgeX = tx + pos.w - 4;
    const badgeY = ty + 4;

    // Video duration badge
    if (item.duration_ms != null) {
      drawBadge(ctx, formatDuration(item.duration_ms), badgeX, badgeY, 'right');
      badgeX -= ctx.measureText(formatDuration(item.duration_ms)).width + 14;
    }

    // Collection member count
    if (item.entity_kind === 'collection' && item.member_count != null) {
      drawBadge(ctx, String(item.member_count), badgeX, badgeY, 'right');
    }

    // Extension badge (bottom-right of image area)
    if (showExtension && item.mime_type) {
      const ext = mimeToExt(item.mime_type);
      if (ext) {
        drawBadge(ctx, ext, tx + pos.w - 4, ty + imgH - BADGE_H - 4, 'right');
      }
    }

    // Rating stars (top-left)
    if (item.rating != null && item.rating > 0) {
      ctx.font = '10px sans-serif';
      ctx.fillStyle = '#ffd54f';
      ctx.textBaseline = 'top';
      ctx.fillText('★'.repeat(item.rating), tx + 5, ty + 5);
    }

    // Name text below image
    if (showName && textHeight > 0 && item.name) {
      ctx.font = NAME_FONT;
      ctx.fillStyle = 'rgba(255, 255, 255, 0.85)';
      ctx.textBaseline = 'top';
      const nameY = ty + imgH + 3;
      const maxNameW = pos.w - 4;
      const displayName = truncateText(ctx, item.name, maxNameW);
      ctx.fillText(displayName, tx + 2, nameY);
    }
  }

  ctx.restore();
}

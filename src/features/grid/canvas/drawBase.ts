/**
 * Canvas base layer drawing — multi-pass renderer.
 *
 * Draws in passes grouped by canvas state to minimize fillStyle/font changes:
 *   Pass 1: Placeholder fills (rounded rects, one fillStyle per tile)
 *   Pass 2: Thumbnails (drawImage only, globalAlpha for reveals)
 *   Pass 3: Batched glass inner borders (single stroke call)
 *   Pass 4: All badges (BADGE_FONT set once)
 *   Pass 5: All rating stars (RATING_FONT set once)
 *   Pass 6: All name text (NAME_FONT set once)
 *
 * Caller sets the transform via ctx.setTransform — no save/restore/scale here.
 */

import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { LayoutItem } from '../layout/types';
import {
  drawImageCover, drawBadge, truncateText, formatDuration, mimeToExt,
  BADGE_FONT, BADGE_H, NAME_FONT, RATING_FONT,
} from './primitives';

const BORDER_RADIUS = 8;
const PLACEHOLDER_COLOR = '#2a2a2e';
const GLASS_BORDER_COLOR = 'rgba(255, 255, 255, 0.15)';

interface CacheEntry { bitmap: ImageBitmap; bytes: number; lastUsedAt: number }

export interface BaseDrawParams {
  ctx: CanvasRenderingContext2D;
  items: CanonicalEntityGridItem[];
  positions: LayoutItem[];
  thumbnails: Map<string, CacheEntry>;
  revealProgressByHash: Map<string, number>;
  textHeight: number;
  visibleStart: number;
  visibleEnd: number;
  showName: boolean;
  showExtension: boolean;
}

export function drawTileMediaLayer(params: BaseDrawParams) {
  const {
    ctx, items, positions, thumbnails, revealProgressByHash,
    textHeight, visibleStart, visibleEnd,
  } = params;

  const end = Math.min(visibleEnd, items.length);

  // ── Pass 1: Placeholder fills (only for tiles without a fully-loaded image) ──
  for (let i = visibleStart; i < end; i++) {
    const item = items[i];
    const pos = positions[i];
    if (!pos) continue;

    const entry = thumbnails.get(item.thumbnail_hash);
    const progress = revealProgressByHash.get(item.thumbnail_hash) ?? 1;

    // Skip placeholder entirely when image is fully revealed
    if (entry && progress >= 1) continue;

    const tx = pos.x | 0;
    const ty = pos.y | 0;
    const tw = pos.w | 0;
    const th = (pos.h - textHeight) | 0;

    ctx.fillStyle = item.dominant_color_hex ?? PLACEHOLDER_COLOR;
    ctx.beginPath();
    ctx.roundRect(tx, ty, tw, th, BORDER_RADIUS);
    ctx.fill();
  }

  // ── Pass 2: Images (clipped to rounded rect) ──────────────────
  for (let i = visibleStart; i < end; i++) {
    const item = items[i];
    const pos = positions[i];
    if (!pos) continue;

    const entry = thumbnails.get(item.thumbnail_hash);
    if (!entry) continue;

    const tx = pos.x | 0;
    const ty = pos.y | 0;
    const tw = pos.w | 0;
    const th = (pos.h - textHeight) | 0;
    const progress = revealProgressByHash.get(item.thumbnail_hash) ?? 1;

    if (progress <= 0) continue;

    ctx.save();
    ctx.beginPath();
    ctx.roundRect(tx, ty, tw, th, BORDER_RADIUS);
    ctx.clip();
    if (progress < 1) {
      ctx.globalAlpha = progress;
      drawImageCover(ctx, entry.bitmap, tx, ty, tw, th);
      ctx.globalAlpha = 1;
    } else {
      drawImageCover(ctx, entry.bitmap, tx, ty, tw, th);
    }
    ctx.restore();
  }

}

export function drawTileChromeLayer(params: BaseDrawParams) {
  const {
    ctx, items, positions, textHeight, visibleStart, visibleEnd, showName, showExtension,
  } = params;

  const end = Math.min(visibleEnd, items.length);

  // ── Pass 3: Batched glass inner borders ──────────────────────
  ctx.strokeStyle = GLASS_BORDER_COLOR;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let i = visibleStart; i < end; i++) {
    const pos = positions[i];
    if (!pos) continue;
    const th = (pos.h - textHeight) | 0;
    ctx.roundRect((pos.x | 0) + 0.5, (pos.y | 0) + 0.5, (pos.w | 0) - 1, th - 1, BORDER_RADIUS - 0.5);
  }
  ctx.stroke();

  // ── Pass 4: All badges (set font once) ───────────────────────
  ctx.font = BADGE_FONT;
  for (let i = visibleStart; i < end; i++) {
    const item = items[i];
    const pos = positions[i];
    if (!pos) continue;

    const tx = pos.x | 0;
    const ty = pos.y | 0;
    const tw = pos.w | 0;
    const th = (pos.h - textHeight) | 0;

    let badgeX = tx + tw - 4;
    const badgeY = ty + 4;

    if (item.duration_ms != null) {
      const durStr = formatDuration(item.duration_ms);
      const w = drawBadge(ctx, durStr, badgeX, badgeY, 'right');
      badgeX -= w + 4;
    }

    if (item.entity_kind === 'collection' && item.member_count != null) {
      drawBadge(ctx, String(item.member_count), badgeX, badgeY, 'right');
    }

    if (showExtension && item.mime_type) {
      const ext = mimeToExt(item.mime_type);
      if (ext) {
        drawBadge(ctx, ext, tx + tw - 4, ty + th - BADGE_H - 4, 'right');
      }
    }
  }

  // ── Pass 5: All rating stars ─────────────────────────────────
  ctx.font = RATING_FONT;
  ctx.fillStyle = '#ffd54f';
  ctx.textBaseline = 'top';
  for (let i = visibleStart; i < end; i++) {
    const item = items[i];
    if (item.rating == null || item.rating <= 0) continue;
    const pos = positions[i];
    if (!pos) continue;
    ctx.fillText('★'.repeat(item.rating), (pos.x | 0) + 5, (pos.y | 0) + 5);
  }

  // ── Pass 6: All name text ────────────────────────────────────
  if (showName && textHeight > 0) {
    ctx.font = NAME_FONT;
    ctx.fillStyle = 'rgba(255, 255, 255, 0.85)';
    ctx.textBaseline = 'top';
    for (let i = visibleStart; i < end; i++) {
      const item = items[i];
      if (!item.name) continue;
      const pos = positions[i];
      if (!pos) continue;
      const tx = pos.x | 0;
      const th = (pos.h - textHeight) | 0;
      const nameY = (pos.y | 0) + th + 3;
      const maxNameW = (pos.w | 0) - 4;
      const displayName = truncateText(ctx, item.name, maxNameW);
      ctx.fillText(displayName, tx + 2, nameY);
    }
  }
}

export function drawBaseLayer(params: BaseDrawParams) {
  drawTileMediaLayer(params);
  drawTileChromeLayer(params);
}

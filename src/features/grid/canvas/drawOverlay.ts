/**
 * Canvas overlay layer — selection borders, hover ring, marquee.
 * Matches legacy canvasGridDrawHelpers.ts drawCanvasOverlayLayer().
 */

import type { LayoutItem } from '../layout/types';

const BORDER_RADIUS = 6;
const SELECTION_COLOR = '#3297FF';
const HOVER_RING_COLOR = 'rgba(50, 151, 255, 0.6)';

export interface OverlayDrawParams {
  ctx: CanvasRenderingContext2D;
  positions: LayoutItem[];
  textHeight: number;
  visibleStart: number;
  visibleEnd: number;
  selectedIndices: Set<number>;
  hoverIndex: number | null;
  dpr: number;
}

export function drawOverlayLayer(params: OverlayDrawParams) {
  const { ctx, positions, textHeight, visibleStart, visibleEnd, selectedIndices, hoverIndex, dpr } = params;

  ctx.save();
  ctx.scale(dpr, dpr);

  for (let i = visibleStart; i < visibleEnd && i < positions.length; i++) {
    const pos = positions[i];
    if (!pos) continue;
    const imgH = pos.h - textHeight;

    // Selection border
    if (selectedIndices.has(i)) {
      ctx.strokeStyle = SELECTION_COLOR;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.roundRect(pos.x + 1, pos.y + 1, pos.w - 2, imgH - 2, BORDER_RADIUS - 1);
      ctx.stroke();
    }

    // Hover ring
    if (hoverIndex === i && !selectedIndices.has(i)) {
      ctx.strokeStyle = HOVER_RING_COLOR;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.roundRect(pos.x + 1, pos.y + 1, pos.w - 2, imgH - 2, BORDER_RADIUS - 1);
      ctx.stroke();
    }
  }

  ctx.restore();
}

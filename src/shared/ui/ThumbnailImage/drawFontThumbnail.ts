import {
  FONT_THUMBNAIL_CARD,
  FONT_THUMBNAIL_BASELINE,
  FONT_THUMBNAIL_FAMILY,
  FONT_THUMBNAIL_GLYPH_GAP,
  FONT_THUMBNAIL_GLYPHS,
  FONT_THUMBNAIL_SIZE,
} from './fontThumbnailGeometry';

/** Canvas counterpart to FontThumbnail. The letters punch through the tile. */
export function drawFontThumbnail(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  cutoutBackground: string,
): void {
  const markSize = Math.min(width * 0.72, height * 0.72, FONT_THUMBNAIL_SIZE);

  context.save();
  context.translate(x + (width - markSize) / 2, y + (height - markSize) / 2);
  context.scale(markSize / FONT_THUMBNAIL_SIZE, markSize / FONT_THUMBNAIL_SIZE);
  context.shadowColor = 'rgba(0,0,0,.24)';
  context.shadowBlur = 6;
  context.shadowOffsetY = 5;
  const fill = context.createLinearGradient(30, 12, 130, 152);
  fill.addColorStop(0, 'rgba(247,248,250,.88)');
  fill.addColorStop(1, 'rgba(167,171,178,.68)');
  context.fillStyle = fill;
  context.beginPath();
  context.roundRect(
    FONT_THUMBNAIL_CARD.x,
    FONT_THUMBNAIL_CARD.y,
    FONT_THUMBNAIL_CARD.size,
    FONT_THUMBNAIL_CARD.size,
    FONT_THUMBNAIL_CARD.radius,
  );
  context.fill();
  context.shadowColor = 'transparent';
  context.globalCompositeOperation = 'destination-out';
  context.fillStyle = '#000';
  context.textBaseline = 'alphabetic';
  context.textAlign = 'left';
  const metrics = FONT_THUMBNAIL_GLYPHS.map((glyph) => {
    context.font = `700 ${glyph.size}px ${FONT_THUMBNAIL_FAMILY}`;
    return context.measureText(glyph.text);
  });
  const widths = metrics.map((metric) => metric.width);
  const totalWidth = widths.reduce((sum, glyphWidth) => sum + glyphWidth, 0)
    + FONT_THUMBNAIL_GLYPH_GAP * (FONT_THUMBNAIL_GLYPHS.length - 1);
  const ascent = Math.max(...metrics.map((metric) => metric.actualBoundingBoxAscent || 0));
  const descent = Math.max(...metrics.map((metric) => metric.actualBoundingBoxDescent || 0));
  const baseline = ascent > 0
    ? FONT_THUMBNAIL_SIZE / 2 + (ascent - descent) / 2
    : FONT_THUMBNAIL_BASELINE;
  const drawGlyphs = () => {
    let glyphX = (FONT_THUMBNAIL_SIZE - totalWidth) / 2;
    FONT_THUMBNAIL_GLYPHS.forEach((glyph, index) => {
      context.font = `700 ${glyph.size}px ${FONT_THUMBNAIL_FAMILY}`;
      context.fillText(glyph.text, glyphX, baseline);
      glyphX += widths[index] + FONT_THUMBNAIL_GLYPH_GAP;
    });
  };
  drawGlyphs();
  // The base canvas already owns the thumbnail background. Repaint that exact
  // value after punching the card out so the holes do not expose the darker
  // application surface underneath the canvas.
  context.globalCompositeOperation = 'source-over';
  context.fillStyle = cutoutBackground;
  drawGlyphs();
  context.restore();
}

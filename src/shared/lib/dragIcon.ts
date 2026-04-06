/**
 * Drag icon generator — creates a thumbnail with count badge for native OS drag.
 * Returns a data URL suitable for Electron's nativeImage.createFromDataURL().
 */

const ICON_SIZE = 64;
const BADGE_HEIGHT = 18;
const BADGE_FONT = 'bold 11px -apple-system, BlinkMacSystemFont, sans-serif';
const BADGE_COLOR = '#ff4757';
const BADGE_TEXT = 'white';
const BADGE_RADIUS = 9;
const BADGE_PAD = 6;

export function createDragIcon(thumbnailUrl: string, count: number): Promise<string> {
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      const canvas = document.createElement('canvas');
      canvas.width = ICON_SIZE;
      canvas.height = ICON_SIZE;
      const ctx = canvas.getContext('2d')!;

      // Draw thumbnail (cover crop to square)
      const s = Math.min(img.width, img.height);
      const sx = (img.width - s) / 2;
      const sy = (img.height - s) / 2;
      ctx.drawImage(img, sx, sy, s, s, 0, 0, ICON_SIZE, ICON_SIZE);

      // Count badge (top-right)
      if (count > 1) {
        const label = count > 999 ? '999+' : String(count);
        ctx.font = BADGE_FONT;
        const textW = ctx.measureText(label).width;
        const badgeW = Math.max(BADGE_HEIGHT, textW + BADGE_PAD * 2);
        const bx = ICON_SIZE - badgeW - 2;
        const by = 2;

        ctx.fillStyle = BADGE_COLOR;
        ctx.beginPath();
        ctx.roundRect(bx, by, badgeW, BADGE_HEIGHT, BADGE_RADIUS);
        ctx.fill();

        ctx.fillStyle = BADGE_TEXT;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(label, bx + badgeW / 2, by + BADGE_HEIGHT / 2);
      }

      resolve(canvas.toDataURL('image/png'));
    };
    img.onerror = () => resolve('');
    img.src = thumbnailUrl;
  });
}

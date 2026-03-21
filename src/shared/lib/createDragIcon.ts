/**
 * Create a drag icon data URL with a thumbnail + count badge.
 *
 * On macOS the icon canvas is double-height with the image in the bottom half,
 * because macOS anchors drag images from the top. The extra space above makes
 * the thumbnail appear below the cursor instead of above it.
 */

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

export function createDragIcon(
  img: HTMLImageElement,
  count: number,
  maxSize = 120,
): string | null {
  try {
    const nw = img.naturalWidth || img.width || 1;
    const nh = img.naturalHeight || img.height || 1;

    const scale = Math.min(maxSize / nw, maxSize / nh, 1);
    const tw = Math.round(nw * scale);
    const th = Math.round(nh * scale);

    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    if (!ctx) return null;

    if (count <= 1) {
      canvas.width = tw;
      canvas.height = th;
      ctx.drawImage(img, 0, 0, tw, th);
    } else {
      const offset = 8;
      canvas.width = tw;
      canvas.height = th;
      ctx.fillStyle = 'rgba(120, 120, 120, 0.5)';
      ctx.fillRect(offset, 0, tw - offset, th - offset);
      ctx.drawImage(img, 0, offset, tw - offset, th - offset);

      const text = count > 9999 ? '9999+' : String(count);
      ctx.font = 'bold 11px -apple-system, "Segoe UI", Arial, sans-serif';
      const metrics = ctx.measureText(text);
      const textW = Math.ceil(metrics.width);
      const pillW = Math.max(textW + 10, 18);
      const pillH = 17;
      const px = canvas.width - pillW - 2;
      const py = 1;

      ctx.beginPath();
      ctx.roundRect(px, py, pillW, pillH, pillH / 2);
      ctx.fillStyle = 'rgba(230, 50, 50, 1)';
      ctx.fill();

      ctx.fillStyle = '#fff';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(text, px + pillW / 2, py + pillH / 2 + 0.5);
    }

    if (isMac) {
      const padded = document.createElement('canvas');
      padded.width = Math.round(2 * canvas.height);
      padded.height = Math.round(2 * canvas.height);
      const pctx = padded.getContext('2d');
      if (pctx) {
        pctx.drawImage(
          canvas,
          padded.width / 2 - canvas.width / 2,
          canvas.height + 12,
          canvas.width,
          canvas.height,
        );
        return padded.toDataURL('image/png');
      }
    }

    return canvas.toDataURL('image/png');
  } catch {
    return null;
  }
}

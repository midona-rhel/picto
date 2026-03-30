/**
 * ColorPalette — row of dominant color swatches in a pill container.
 * Always reserves the same height (28px) even when empty, so layout doesn't shift.
 * Order is preserved from the database (= relevance/coverage order).
 */

import { useState, useCallback } from 'react';
import styles from './ColorPalette.module.css';

interface ColorPaletteProps {
  /** Hex color strings in relevance order (most coverage first). */
  colors: string[];
}

export function ColorPalette({ colors }: ColorPaletteProps) {
  const [copiedColor, setCopiedColor] = useState<string | null>(null);

  const handleCopy = useCallback((hex: string) => {
    navigator.clipboard.writeText(hex).then(() => {
      setCopiedColor(hex);
      setTimeout(() => setCopiedColor(null), 1500);
    }).catch(() => {});
  }, []);

  // Always render container to reserve height — hidden when empty
  return (
    <div className={styles.palette} style={colors.length === 0 ? { visibility: 'hidden' } : undefined}>
      {(colors.length === 0 ? ['#000000'] : colors).map((hex, i) => (
        <div
          key={i}
          className={styles.swatchWrap}
          title={copiedColor === hex ? 'Copied!' : `${hex} · Click to copy`}
          onClick={() => handleCopy(hex)}
        >
          <div className={styles.swatch} style={{ backgroundColor: hex }} />
        </div>
      ))}
    </div>
  );
}

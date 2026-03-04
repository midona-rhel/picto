import { useState, useCallback } from 'react';
import styles from './ColorPalette.module.css';

interface ColorPaletteProps {
  colors: Array<{ hex: string }>;
}

export function ColorPalette({ colors }: ColorPaletteProps) {
  const [copiedColor, setCopiedColor] = useState<string | null>(null);

  const handleClick = useCallback((hex: string) => {
    navigator.clipboard.writeText(hex).then(() => {
      setCopiedColor(hex);
      setTimeout(() => setCopiedColor(null), 1500);
    }).catch(() => {});
  }, []);

  if (colors.length === 0) {
    return <div className={styles.colorPalette} style={{ visibility: 'hidden' }} />;
  }

  return (
    <div className={styles.colorPalette}>
      {colors.map((color, idx) => (
        <div
          key={idx}
          className={styles.colorSwatchWrap}
          title={copiedColor === color.hex ? 'Copied!' : color.hex}
        >
          <div
            className={styles.colorSwatch}
            style={{ backgroundColor: color.hex }}
            onClick={() => handleClick(color.hex)}
          />
        </div>
      ))}
    </div>
  );
}

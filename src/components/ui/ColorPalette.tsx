import { useState, useCallback, useMemo } from 'react';
import { IconCopy, IconRefresh, IconSearch } from '@tabler/icons-react';
import { ContextMenu, useContextMenu, type ContextMenuEntry } from './ContextMenu';
import styles from './ColorPalette.module.css';

interface ColorPaletteProps {
  colors: Array<{ hex: string }>;
  onFindSimilarColor?: (hex: string) => void;
  onReanalyzeColors?: () => void;
}

function hexToHslSortKey(hex: string): [number, number, number] {
  const clean = hex.replace('#', '');
  if (!/^[0-9a-fA-F]{6}$/.test(clean)) return [360, 0, 0];
  const r = parseInt(clean.slice(0, 2), 16) / 255;
  const g = parseInt(clean.slice(2, 4), 16) / 255;
  const b = parseInt(clean.slice(4, 6), 16) / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return [0, 0, l];
  const s = d / (1 - Math.abs(2 * l - 1));
  let h = 0;
  if (max === r) h = 60 * (((g - b) / d) % 6);
  else if (max === g) h = 60 * ((b - r) / d + 2);
  else h = 60 * ((r - g) / d + 4);
  if (h < 0) h += 360;
  return [h, s, l];
}

export function ColorPalette({ colors, onFindSimilarColor, onReanalyzeColors }: ColorPaletteProps) {
  const [copiedColor, setCopiedColor] = useState<string | null>(null);
  const contextMenu = useContextMenu();

  const sortedColors = useMemo(() => {
    return [...colors].sort((a, b) => {
      const [ha, sa, la] = hexToHslSortKey(a.hex);
      const [hb, sb, lb] = hexToHslSortKey(b.hex);
      if (ha !== hb) return ha - hb;
      if (sa !== sb) return sa - sb;
      return la - lb;
    });
  }, [colors]);

  const handleCopyHex = useCallback((hex: string) => {
    navigator.clipboard.writeText(hex).then(() => {
      setCopiedColor(hex);
      setTimeout(() => setCopiedColor(null), 1500);
    }).catch(() => {});
  }, []);

  const handleFindSimilar = useCallback((hex: string) => {
    onFindSimilarColor?.(hex);
  }, [onFindSimilarColor]);

  const handleSwatchContextMenu = useCallback((e: React.MouseEvent, hex: string) => {
    const items: ContextMenuEntry[] = [
      {
        type: 'item',
        label: 'Copy Hex',
        icon: <IconCopy size={14} />,
        onClick: () => handleCopyHex(hex),
      },
      {
        type: 'item',
        label: 'Find Similar Colors',
        icon: <IconSearch size={14} />,
        onClick: () => handleFindSimilar(hex),
        disabled: !onFindSimilarColor,
      },
    ];
    if (onReanalyzeColors) {
      items.push({ type: 'separator' });
      items.push({
        type: 'item',
        label: 'Re-analyze Colors',
        icon: <IconRefresh size={14} />,
        onClick: onReanalyzeColors,
      });
    }
    contextMenu.open(e, items);
  }, [contextMenu, handleCopyHex, handleFindSimilar, onFindSimilarColor, onReanalyzeColors]);

  if (colors.length === 0) {
    return <div className={styles.colorPalette} style={{ visibility: 'hidden' }} />;
  }

  return (
    <div className={styles.colorPalette}>
      {sortedColors.map((color, idx) => (
        <div
          key={idx}
          className={styles.colorSwatchWrap}
          title={
            copiedColor === color.hex
              ? 'Copied!'
              : `${color.hex} · Right-click for actions`
          }
        >
          <div
            className={styles.colorSwatch}
            style={{ backgroundColor: color.hex }}
            onContextMenu={(e) => handleSwatchContextMenu(e, color.hex)}
          />
        </div>
      ))}
      {contextMenu.state && (
        <ContextMenu
          items={contextMenu.state.items}
          position={contextMenu.state.position}
          onClose={() => {
            contextMenu.close();
          }}
          searchable={false}
          panelWidth={196}
        />
      )}
    </div>
  );
}

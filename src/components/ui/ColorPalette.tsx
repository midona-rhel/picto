import { useState, useCallback } from 'react';
import { IconCopy, IconSearch } from '@tabler/icons-react';
import { ContextMenu, useContextMenu, type ContextMenuEntry } from './ContextMenu';
import styles from './ColorPalette.module.css';

interface ColorPaletteProps {
  colors: Array<{ hex: string }>;
  onFindSimilarColor?: (hex: string) => void;
}

export function ColorPalette({ colors, onFindSimilarColor }: ColorPaletteProps) {
  const [copiedColor, setCopiedColor] = useState<string | null>(null);
  const contextMenu = useContextMenu();

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
    contextMenu.open(e, items);
  }, [contextMenu, handleCopyHex, handleFindSimilar, onFindSimilarColor]);

  if (colors.length === 0) {
    return <div className={styles.colorPalette} style={{ visibility: 'hidden' }} />;
  }

  return (
    <div className={styles.colorPalette}>
      {colors.map((color, idx) => (
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

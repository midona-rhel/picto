/**
 * ColorPalette — row of dominant color swatches in a pill container.
 * Always reserves the same height (28px) even when empty, so layout doesn't shift.
 * Order is preserved from the database (= relevance/coverage order).
 */

import { useState, useCallback } from 'react';
import { IconCopy, IconFilter } from '@tabler/icons-react';
import { ContextMenu, type MenuEntry, useContextMenu } from '../ContextMenu/ContextMenu';
import { KbdTooltip } from '../KbdTooltip';
import styles from './ColorPalette.module.css';
import { t } from '../../../i18n';

interface ColorPaletteProps {
  /** Hex color strings in relevance order (most coverage first). */
  colors: string[];
  /** Applies the grid color filter represented by this swatch. */
  onFilter?: (hex: string) => void;
}

function hexToRgb(hex: string): [number, number, number] {
  const value = hex.replace('#', '');
  return [0, 2, 4].map((offset) => Number.parseInt(value.slice(offset, offset + 2), 16)) as [number, number, number];
}

function rgbToHsl([rByte, gByte, bByte]: [number, number, number]): string {
  const [r, g, b] = [rByte, gByte, bByte].map((value) => value / 255);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const lightness = (max + min) / 2;
  if (max === min) return `hsl(0, 0%, ${Math.round(lightness * 100)}%)`;
  const delta = max - min;
  const saturation = lightness > 0.5 ? delta / (2 - max - min) : delta / (max + min);
  const hue = max === r
    ? ((g - b) / delta + (g < b ? 6 : 0)) / 6
    : max === g
      ? ((b - r) / delta + 2) / 6
      : ((r - g) / delta + 4) / 6;
  return `hsl(${Math.round(hue * 360)}, ${Math.round(saturation * 100)}%, ${Math.round(lightness * 100)}%)`;
}

function rgbToHsv([rByte, gByte, bByte]: [number, number, number]): string {
  const [r, g, b] = [rByte, gByte, bByte].map((value) => value / 255);
  const max = Math.max(r, g, b);
  const delta = max - Math.min(r, g, b);
  const hue = delta === 0 ? 0
    : max === r ? 60 * (((g - b) / delta) % 6)
      : max === g ? 60 * ((b - r) / delta + 2)
        : 60 * ((r - g) / delta + 4);
  return `hsv(${Math.round(hue < 0 ? hue + 360 : hue)}, ${Math.round((max === 0 ? 0 : delta / max) * 100)}%, ${Math.round(max * 100)}%)`;
}

function rgbToHwb(rgb: [number, number, number]): string {
  const hsv = rgbToHsv(rgb);
  const hue = hsv.slice(4, hsv.indexOf(','));
  const [r, g, b] = rgb.map((value) => value / 255);
  return `hwb(${hue} ${Math.round(Math.min(r, g, b) * 100)}% ${Math.round((1 - Math.max(r, g, b)) * 100)}%)`;
}

function rgbToCmyk([rByte, gByte, bByte]: [number, number, number]): string {
  const [r, g, b] = [rByte, gByte, bByte].map((value) => value / 255);
  const key = 1 - Math.max(r, g, b);
  if (key === 1) return 'cmyk(0%, 0%, 0%, 100%)';
  const component = (value: number) => Math.round(((1 - value - key) / (1 - key)) * 100);
  return `cmyk(${component(r)}%, ${component(g)}%, ${component(b)}%, ${Math.round(key * 100)}%)`;
}

function colorCopyEntries(hex: string, copy: (value: string) => void): MenuEntry[] {
  const rgb = hexToRgb(hex);
  const [r, g, b] = rgb;
  return [
    { label: t("Copy HEX"), icon: <IconCopy size={15} />, action: () => copy(hex.toUpperCase()) },
    { label: t("Copy RGB"), icon: <IconCopy size={15} />, action: () => copy(`rgb(${r}, ${g}, ${b})`) },
    { label: t("Copy RGBA"), icon: <IconCopy size={15} />, action: () => copy(`rgba(${r}, ${g}, ${b}, 1)`) },
    { label: t("Copy HSL"), icon: <IconCopy size={15} />, action: () => copy(rgbToHsl(rgb)) },
    { separator: true },
    { label: t("Copy HSV"), icon: <IconCopy size={15} />, action: () => copy(rgbToHsv(rgb)) },
    { label: t("Copy HWB"), icon: <IconCopy size={15} />, action: () => copy(rgbToHwb(rgb)) },
    { label: t("Copy CMYK"), icon: <IconCopy size={15} />, action: () => copy(rgbToCmyk(rgb)) },
  ];
}

export function ColorPalette({ colors, onFilter }: ColorPaletteProps) {
  const [copiedColor, setCopiedColor] = useState<string | null>(null);
  const menu = useContextMenu();

  const handleCopy = useCallback((value: string, sourceHex = value) => {
    navigator.clipboard.writeText(value).then(() => {
      setCopiedColor(sourceHex);
      setTimeout(() => setCopiedColor(null), 1500);
    }).catch(() => {});
  }, []);

  const openMenu = useCallback((event: React.MouseEvent, hex: string) => {
    const entries: MenuEntry[] = [
      ...(onFilter ? [{
        label: t("Filter by color"),
        icon: <IconFilter size={15} />,
        action: () => onFilter(hex),
      } satisfies MenuEntry, { separator: true } satisfies MenuEntry] : []),
      ...colorCopyEntries(hex, (value) => handleCopy(value, hex)),
    ];
    menu.open(event, entries, { showSearch: false });
  }, [handleCopy, menu, onFilter]);

  // Always render container to reserve height — hidden when empty
  return (
    <div className={styles.palette} style={colors.length === 0 ? { visibility: 'hidden' } : undefined}>
      {(colors.length === 0 ? ['#000000'] : colors).map((hex, i) => (
        <KbdTooltip key={i} label={copiedColor === hex ? t("Copied!") : t("{value0} · Click to filter · Right-click for actions", { value0: hex })}><div
          className={styles.swatchWrap}
          onClick={() => onFilter?.(hex)}
          onContextMenu={(event) => {
            event.preventDefault();
            event.stopPropagation();
            openMenu(event, hex);
          }}
        >
          <div className={styles.swatch} style={{ backgroundColor: hex }} />
        </div></KbdTooltip>
      ))}
      {menu.state && (
        <ContextMenu
          entries={menu.state.entries}
          position={menu.state.position}
          showSearch={menu.state.showSearch}
          onClose={menu.close}
        />
      )}
    </div>
  );
}

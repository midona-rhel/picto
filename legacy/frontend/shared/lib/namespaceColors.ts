type RGB = [number, number, number];

const NAMESPACE_COLORS: Record<string, RGB> = {
  creator:   [170, 0, 0],
  studio:    [128, 0, 0],
  character: [0, 170, 0],
  person:    [0, 128, 0],
  series:    [170, 0, 170],
  species:   [0, 130, 170],
  meta:      [0, 0, 0],
  system:    [153, 101, 21],
  '':        [114, 160, 193],
};

const UNNAMESPACED_COLOR: RGB = [114, 160, 193];

export function getNamespaceColor(namespace: string, isDark: boolean): RGB {
  const key = namespace.toLowerCase();
  const color = NAMESPACE_COLORS[key] ?? UNNAMESPACED_COLOR;
  // In dark mode, pure black is invisible — use light gray instead
  if (isDark && color[0] === 0 && color[1] === 0 && color[2] === 0) {
    return [160, 160, 160];
  }
  return color;
}

export function chipStyleFromRgb(
  rgb: RGB,
  isDark: boolean,
): { background: string; color: string; border: string } {
  const [r, g, b] = rgb;
  return {
    background: isDark
      ? `rgba(${r}, ${g}, ${b}, 0.12)`
      : `rgba(${r}, ${g}, ${b}, 0.10)`,
    color: isDark
      ? 'rgba(255, 255, 255, 0.85)'
      : 'rgba(0, 0, 0, 0.78)',
    border: `1px solid rgba(${r}, ${g}, ${b}, 0.25)`,
  };
}

export function namespaceChipStyle(
  namespace: string,
  isDark: boolean,
): { background: string; color: string; border: string } {
  const rgb = getNamespaceColor(namespace, isDark);
  return chipStyleFromRgb(rgb, isDark);
}

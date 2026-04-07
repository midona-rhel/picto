/**
 * Dynamic icon resolver — maps icon name strings to Tabler icon components.
 *
 * Uses the same ICON_MAP as IconPicker so every selectable icon renders correctly.
 * System scope icons are used directly as JSX in the feature root.
 */

import { IconFolder } from '@tabler/icons-react';
import { ICON_MAP } from './IconPicker/iconRegistry';

interface DynamicIconProps {
  name: string;
  size?: number;
  color?: string | null;
  stroke?: number;
  filled?: boolean;
}

export function DynamicIcon({ name, size = 16, color, stroke = 1.2, filled }: DynamicIconProps) {
  const Icon = ICON_MAP.get(name) ?? IconFolder;
  if (filled) {
    return (
      <Icon
        size={size}
        stroke={stroke}
        fill={color ?? 'currentColor'}
        fillOpacity={0.15}
        color={color ?? undefined}
      />
    );
  }
  return <Icon size={size} stroke={stroke} color={color ?? undefined} />;
}

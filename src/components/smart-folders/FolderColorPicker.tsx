import { useState } from 'react';
import { Group, ColorSwatch } from '@mantine/core';
import { IconCheck } from '@tabler/icons-react';

/** Sentinel for the "Default" swatch — inherits text color (white in dark, black in light). */
export const FOLDER_COLOR_DEFAULT = null;

export const FOLDER_COLORS: { name: string; hex: string | null }[] = [
  { name: 'Default', hex: FOLDER_COLOR_DEFAULT },
  { name: 'Red', hex: '#FA5252' },
  { name: 'Orange', hex: '#FD7E14' },
  { name: 'Yellow', hex: '#FAB005' },
  { name: 'Green', hex: '#40C057' },
  { name: 'Teal', hex: '#12B886' },
  { name: 'Blue', hex: '#339AF0' },
  { name: 'Indigo', hex: '#5C7CFA' },
  { name: 'Purple', hex: '#7950F2' },
  { name: 'Pink', hex: '#E64980' },
];

interface FolderColorPickerProps {
  value: string | null;
  onChange: (hex: string | null) => void;
}

export function FolderColorPicker({ value, onChange }: FolderColorPickerProps) {
  const [local, setLocal] = useState(value);

  const handleClick = (hex: string | null) => {
    setLocal(hex);
    onChange(hex);
  };

  return (
    <Group gap={4}>
      {FOLDER_COLORS.map((c) => {
        const isSelected = local === c.hex;
        return (
          <ColorSwatch
            key={c.hex ?? 'default'}
            color={c.hex ?? 'var(--color-text-primary)'}
            size={20}
            onClick={() => handleClick(c.hex)}
            style={{ cursor: 'pointer' }}
          >
            {isSelected && <IconCheck size={12} color={c.hex ? 'white' : 'var(--color-bg-primary, #1a1b1e)'} stroke={3} />}
          </ColorSwatch>
        );
      })}
    </Group>
  );
}

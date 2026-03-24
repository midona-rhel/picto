/**
 * Inline color swatch picker — row of colored circles with inset ring selection.
 */

import { useState } from 'react';
import { IconCheck } from '@tabler/icons-react';
import styles from './ColorPicker.module.css';

export const FOLDER_COLORS: { name: string; hex: string | null }[] = [
  { name: 'Default', hex: null },
  { name: 'Blue', hex: '#339AF0' },
  { name: 'Red', hex: '#FA5252' },
  { name: 'Orange', hex: '#FD7E14' },
  { name: 'Yellow', hex: '#FAB005' },
  { name: 'Green', hex: '#40C057' },
  { name: 'Teal', hex: '#12B886' },
  { name: 'Indigo', hex: '#5C7CFA' },
  { name: 'Purple', hex: '#7950F2' },
  { name: 'Pink', hex: '#E64980' },
];

interface ColorPickerProps {
  value: string | null;
  onChange: (hex: string | null) => void;
}

export function ColorPicker({ value, onChange }: ColorPickerProps) {
  const [local, setLocal] = useState(value);

  const handleClick = (hex: string | null) => {
    setLocal(hex);
    onChange(hex);
  };

  return (
    <div className={styles.row}>
      {FOLDER_COLORS.map((c) => {
        const isSelected = local === c.hex;
        return (
          <button
            key={c.hex ?? 'default'}
            className={`${styles.swatch} ${isSelected ? styles.selected : ''}`}
            style={{
              backgroundColor: c.hex ?? 'var(--color-text-primary)',
              '--swatch-color': c.hex ?? 'var(--color-text-primary)',
            } as React.CSSProperties}
            title={c.name}
            onClick={() => handleClick(c.hex)}
          >
            {isSelected && (
              <IconCheck
                size={10}
                stroke={2.5}
                color={c.hex ? 'white' : 'var(--color-bg-app)'}
              />
            )}
          </button>
        );
      })}
    </div>
  );
}

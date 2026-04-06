/**
 * Icon picker — flat grid with search, no category headers.
 * Used inside context menu submenus and property panels.
 */

import { useState, useMemo } from 'react';
import { IconRotate2 } from '@tabler/icons-react';
import { CURATED_ICONS } from './iconRegistry';
import styles from './IconPicker.module.css';

const ICON_SIZE = 19;

interface IconPickerProps {
  value: string | null;
  onChange: (icon: string | null) => void;
}

export function IconPicker({ value, onChange }: IconPickerProps) {
  const [local, setLocal] = useState(value);
  const [search, setSearch] = useState('');

  const query = search.toLowerCase().trim();
  const filtered = useMemo(
    () => query
      ? CURATED_ICONS.filter((i) => i.label.toLowerCase().includes(query) || i.name.toLowerCase().includes(query))
      : CURATED_ICONS,
    [query],
  );

  const handleSelect = (iconName: string | null) => {
    setLocal(iconName);
    onChange(iconName);
  };

  return (
    <div className={styles.root} onClick={(e) => e.stopPropagation()}>
      <div className={styles.searchRow}>
        <input
          type="text"
          placeholder="Search icons..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          autoFocus
          className={styles.searchInput}
        />
        <button
          title="Reset to default"
          disabled={!local}
          onClick={() => handleSelect(null)}
          className={`${styles.resetBtn} ${local ? '' : styles.resetDisabled}`}
        >
          <IconRotate2 size={12} />
        </button>
      </div>

      <div className={styles.grid}>
        {filtered.length === 0 && (
          <div className={styles.empty}>No icons found</div>
        )}
        {filtered.map((icon) => {
          const Icon = icon.component;
          const isSelected = local === icon.name;
          return (
            <button
              key={icon.name}
              title={icon.label}
              onClick={() => handleSelect(icon.name)}
              className={`${styles.iconBtn} ${isSelected ? styles.iconSelected : ''}`}
            >
              <Icon size={ICON_SIZE} stroke={1.5} />
            </button>
          );
        })}
      </div>
    </div>
  );
}

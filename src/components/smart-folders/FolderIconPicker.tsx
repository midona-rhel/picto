import { useState, useMemo } from 'react';
import { IconRotate2 } from '@tabler/icons-react';
import { CURATED_ICONS, ICON_CATEGORIES, type CuratedIcon } from './iconRegistry';

const GRID_COLS = 8;
const ICON_SIZE = 14;
const BTN_SIZE = 26;
const MAX_HEIGHT = 200;

interface FolderIconPickerProps {
  value: string | null;
  onChange: (icon: string | null) => void;
}

/**
 * Compact inline icon picker for context menus.
 * Shows a search bar + scrollable categorised grid.
 */
export function FolderIconPicker({ value, onChange }: FolderIconPickerProps) {
  const [local, setLocal] = useState(value);
  const [search, setSearch] = useState('');

  const query = search.toLowerCase().trim();

  const filtered: CuratedIcon[] | null = useMemo(
    () =>
      query
        ? CURATED_ICONS.filter(
            (i) =>
              i.label.toLowerCase().includes(query) ||
              i.name.toLowerCase().includes(query) ||
              i.category.toLowerCase().includes(query),
          )
        : null,
    [query],
  );

  const handleSelect = (iconName: string | null) => {
    setLocal(iconName);
    onChange(iconName);
  };

  const renderGrid = (icons: CuratedIcon[]) => (
    <div style={{ display: 'grid', gridTemplateColumns: `repeat(${GRID_COLS}, ${BTN_SIZE}px)`, gap: 2 }}>
      {icons.map((icon) => {
        const Icon = icon.component;
        const isSelected = local === icon.name;
        return (
          <button
            key={icon.name}
            title={icon.label}
            onClick={() => handleSelect(icon.name)}
            style={{
              width: BTN_SIZE,
              height: BTN_SIZE,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              border: 'none',
              borderRadius: 4,
              cursor: 'pointer',
              background: isSelected ? 'var(--color-white-10, rgba(255,255,255,0.1))' : 'transparent',
              color: isSelected ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
            }}
          >
            <Icon size={ICON_SIZE} />
          </button>
        );
      })}
    </div>
  );

  return (
    <div style={{ width: GRID_COLS * (BTN_SIZE + 2) + 8 }} onClick={(e) => e.stopPropagation()}>
      <div style={{ display: 'flex', gap: 4, marginBottom: 6 }}>
        <input
          type="text"
          placeholder="Search icons..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          autoFocus
          style={{
            flex: 1,
            minWidth: 0,
            height: 24,
            fontSize: 12,
            padding: '0 6px',
            border: '1px solid var(--color-border-primary)',
            borderRadius: 4,
            background: 'var(--color-white-10)',
            color: 'var(--color-text-primary)',
            outline: 'none',
            boxSizing: 'border-box',
          }}
        />
        <button
          title="Reset to default"
          disabled={!local}
          onClick={() => handleSelect(null)}
          style={{
            width: 24,
            height: 24,
            flexShrink: 0,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            border: 'none',
            borderRadius: 4,
            cursor: local ? 'pointer' : 'default',
            background: 'var(--color-white-10, rgba(255,255,255,0.1))',
            color: local ? 'var(--color-text-primary)' : 'var(--color-text-tertiary)',
            opacity: local ? 1 : 0.4,
          }}
        >
          <IconRotate2 size={12} />
        </button>
      </div>

      <div style={{ maxHeight: MAX_HEIGHT, overflowY: 'auto', overflowX: 'hidden' }}>
        {filtered ? (
          filtered.length > 0 ? (
            renderGrid(filtered)
          ) : (
            <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', textAlign: 'center', padding: '8px 0' }}>
              No icons found
            </div>
          )
        ) : (
          ICON_CATEGORIES.map((category) => {
            const icons = CURATED_ICONS.filter((i) => i.category === category);
            return (
              <div key={category}>
                <div style={{ fontSize: 10, color: 'var(--color-text-tertiary)', fontWeight: 500, padding: '6px 0 2px' }}>
                  {category}
                </div>
                {renderGrid(icons)}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

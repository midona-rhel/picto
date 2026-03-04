import { useState } from 'react';
import { Select } from '@mantine/core';
import { IconSortAscending, IconSortDescending } from '@tabler/icons-react';
import { cmSelectInput, cmSelectDropdown, cmSelectOption, cmComboboxProps } from '../ui/cmSelectStyles';

export function SortByRow({ field, order, onFieldChange, onOrderChange }: {
  field: string; order: string;
  onFieldChange: (f: string) => void; onOrderChange: (o: string) => void;
}) {
  const [localField, setLocalField] = useState(field);
  const [localOrder, setLocalOrder] = useState(order);
  const handleFieldChange = (v: string | null) => { if (v) { setLocalField(v); onFieldChange(v); } };
  const toggle = (o: string) => { setLocalOrder(o); onOrderChange(o); };
  const btnStyle = (active: boolean): React.CSSProperties => ({
    display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
    width: 28, height: 26, borderRadius: 6, cursor: 'pointer', border: 'none',
    background: active ? 'var(--color-white-15)' : 'transparent',
    color: active ? 'var(--color-text-primary)' : 'var(--mantine-color-dimmed)',
  });
  return (
    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
      <span style={{ color: 'var(--mantine-color-text)', fontSize: 'var(--mantine-font-size-sm)' }}>Sort by</span>
      <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        <Select
          size="xs"
          value={localField}
          onChange={handleFieldChange}
          data={[
            { value: 'imported_at', label: 'Date Added' },
            { value: 'size', label: 'File Size' },
            { value: 'rating', label: 'Rating' },
            { value: 'view_count', label: 'Views' },
          ]}
          allowDeselect={false}
          withCheckIcon={false}
          rightSectionWidth={20}
          comboboxProps={cmComboboxProps}
          styles={{
            input: { ...cmSelectInput },
            wrapper: { width: 120 },
            dropdown: cmSelectDropdown,
            option: cmSelectOption,
            section: { color: 'var(--color-text-primary)' },
          }}
        />
        <div style={{
            display: 'flex', alignItems: 'center', gap: 0,
            background: 'var(--color-white-05)', borderRadius: 6,
            padding: 1,
          }}>
            <button style={btnStyle(localOrder === 'asc')} onClick={() => toggle('asc')}>
              <IconSortAscending size={16} />
            </button>
            <button style={btnStyle(localOrder === 'desc')} onClick={() => toggle('desc')}>
              <IconSortDescending size={16} />
            </button>
          </div>
      </div>
    </div>
  );
}

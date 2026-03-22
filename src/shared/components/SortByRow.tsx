import { useState } from 'react';
import { Select } from '@mantine/core';
import { IconSortAscending, IconSortDescending } from '@tabler/icons-react';
import { cmSelectInput, cmSelectDropdown, cmSelectOption, cmComboboxProps } from './cmSelectStyles';

export function SortByRow({ field, order, onFieldChange, onOrderChange }: {
  field: string; order: string;
  onFieldChange: (f: string) => void; onOrderChange: (o: string) => void;
}) {
  const [localField, setLocalField] = useState(field || 'date_added');
  const [localOrder, setLocalOrder] = useState(order || 'desc');
  const handleFieldChange = (v: string | null) => { if (v) { setLocalField(v); onFieldChange(v); } };
  const handleOrderChange = (o: string) => { setLocalOrder(o); onOrderChange(o); };
  const btnStyle = (active: boolean): React.CSSProperties => ({
    display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
    width: 28, height: 26, borderRadius: 5, cursor: 'pointer', border: 'none',
    background: active ? 'var(--color-black-10, rgba(0, 0, 0, 0.1))' : 'transparent',
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
            { value: 'date_added', label: 'Date Added' },
            { value: 'date_created', label: 'Date Created' },
            { value: 'date_modified', label: 'Date Modified' },
            { value: 'size', label: 'File Size' },
            { value: 'rating', label: 'Rating' },
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
            background: 'var(--color-black-05, rgba(0, 0, 0, 0.05))', borderRadius: 6,
            padding: 1,
          }}>
            <button style={btnStyle(localOrder === 'asc')} onClick={() => handleOrderChange('asc')}>
              <IconSortAscending size={16} />
            </button>
            <button style={btnStyle(localOrder === 'desc')} onClick={() => handleOrderChange('desc')}>
              <IconSortDescending size={16} />
            </button>
          </div>
      </div>
    </div>
  );
}

import { useState } from 'react';
import { Select } from '@mantine/core';
import { IconBorderAll, IconLayoutBoard } from '@tabler/icons-react';
import { cmSelectInput, cmSelectDropdown, cmSelectOption, cmComboboxProps } from '../../shared/components/cmSelectStyles';
import type { GridViewMode } from './runtime';

function ModeIcon({ mode, size = 14 }: { mode: GridViewMode; size?: number }) {
  if (mode === 'grid') return <IconBorderAll size={size} />;
  if (mode === 'justified') return <IconLayoutBoard size={size} style={{ transform: 'rotate(-90deg)' }} />;
  return <IconLayoutBoard size={size} />;
}

export function LayoutRow({ viewMode, onChange }: { viewMode: GridViewMode; onChange: (m: GridViewMode) => void }) {
  const [local, setLocal] = useState(viewMode);
  const handleChange = (v: string | null) => { if (v) { setLocal(v as GridViewMode); onChange(v as GridViewMode); } };
  return (
    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
      <span style={{ color: 'var(--mantine-color-text)', fontSize: 'var(--mantine-font-size-sm)' }}>Layout</span>
      <Select
        size="xs"
        value={local}
        onChange={handleChange}
        data={[
          { value: 'waterfall', label: 'Waterfall' },
          { value: 'grid', label: 'Grid' },
          { value: 'justified', label: 'Justified' },
        ]}
        allowDeselect={false}
        withCheckIcon={false}
        leftSection={<ModeIcon mode={local} size={14} />}
        leftSectionPointerEvents="none"
        leftSectionWidth={36}
        rightSectionWidth={20}
        comboboxProps={cmComboboxProps}
        styles={{
          input: { ...cmSelectInput, paddingLeft: 33 },
          wrapper: { width: 120 },
          dropdown: cmSelectDropdown,
          option: cmSelectOption,
          section: { color: 'var(--color-text-primary)' },
        }}
        renderOption={({ option }) => (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, pointerEvents: 'none', color: 'var(--color-text-primary)' }}>
            <ModeIcon mode={option.value as GridViewMode} size={14} />
            <span>{option.label}</span>
          </div>
        )}
      />
    </div>
  );
}

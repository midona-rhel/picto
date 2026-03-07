import { useRef, useState } from 'react';
import { TextInput, NumberInput, Switch, Group, Select, ColorSwatch, SimpleGrid, ActionIcon } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import { IconPlus } from '@tabler/icons-react';
import { NamespaceTagChip } from '../../../shared/components/NamespaceTagChip';
import type { FieldConfig } from './fieldConfig';
import type { SmartRule } from './types';
import { TagPickerMenu } from './TagPickerMenu';

interface ValuePickerProps {
  config: FieldConfig;
  rule: SmartRule;
  onChange: (partial: Partial<SmartRule>) => void;
}

const SIZE_UNITS = [
  { value: '1', label: 'B' },
  { value: '1024', label: 'KB' },
  { value: '1048576', label: 'MB' },
  { value: '1073741824', label: 'GB' },
];

const DATE_DURATION_UNITS = [
  { value: 'd', label: 'days' },
  { value: 'w', label: 'weeks' },
  { value: 'm', label: 'months' },
  { value: 'y', label: 'years' },
];

const COLOR_SWATCHES = [
  '#FF0000', '#FF6600', '#FFCC00', '#00CC00', '#0066FF',
  '#9933FF', '#FF0099', '#FFFFFF', '#999999', '#000000',
  '#CC3300', '#FF9900', '#FFFF00', '#33CC33', '#0099FF',
  '#6600CC', '#FF3399', '#FFCCCC', '#CCCCCC', '#333333',
];

/** No-value operators that don't need a picker */
const NO_VALUE_OPS = ['is_empty', 'is_not_empty', 'is_set', 'is_not_set'];

export function ValuePicker({ config, rule, onChange }: ValuePickerProps) {
  if (NO_VALUE_OPS.includes(rule.op)) {
    return null;
  }

  const isBetween = rule.op === 'between';
  const isInLast = rule.op === 'in_last' || rule.op === 'not_in_last';

  switch (config.valueType) {
    case 'tags':
      return <TagValuePicker rule={rule} onChange={onChange} />;

    case 'text':
      return (
        <TextInput
          size="xs"
          placeholder="value..."
          value={String(rule.value ?? '')}
          onChange={(e) => onChange({ value: e.currentTarget.value })}
          style={{ flex: 1, minWidth: 120 }}
        />
      );

    case 'number':
      return (
        <Group gap={4} style={{ flex: 1 }}>
          <NumberInput
            size="xs"
            value={Number(rule.value) || 0}
            onChange={(v) => onChange({ value: Number(v) })}
            style={{ width: isBetween ? 80 : 100 }}
            rightSection={config.unit ? <span style={{ fontSize: 10, color: 'var(--mantine-color-dimmed)' }}>{config.unit}</span> : undefined}
          />
          {isBetween && (
            <>
              <span style={{ fontSize: 'var(--mantine-font-size-xs)', color: 'var(--mantine-color-dimmed)' }}>and</span>
              <NumberInput
                size="xs"
                value={Number(rule.value2) || 0}
                onChange={(v) => onChange({ value2: Number(v) })}
                style={{ width: 80 }}
                rightSection={config.unit ? <span style={{ fontSize: 10, color: 'var(--mantine-color-dimmed)' }}>{config.unit}</span> : undefined}
              />
            </>
          )}
        </Group>
      );

    case 'filesize':
      return <FileSizeValuePicker rule={rule} onChange={onChange} isBetween={isBetween} />;

    case 'date':
      if (isInLast) {
        return <DateDurationPicker rule={rule} onChange={onChange} />;
      }
      if (isBetween) {
        return (
          <Group gap={4} style={{ flex: 1 }}>
            <DatePickerInput
              size="xs"
              value={rule.value ? new Date(String(rule.value)) : null}
              onChange={(d: unknown) => onChange({ value: d ? (d instanceof Date ? d.toISOString() : String(d)) : undefined })}
              style={{ width: 130 }}
              placeholder="from"
            />
            <span style={{ fontSize: 'var(--mantine-font-size-xs)', color: 'var(--mantine-color-dimmed)' }}>and</span>
            <DatePickerInput
              size="xs"
              value={rule.value2 ? new Date(String(rule.value2)) : null}
              onChange={(d: unknown) => onChange({ value2: d ? (d instanceof Date ? d.toISOString() : String(d)) : undefined })}
              style={{ width: 130 }}
              placeholder="to"
            />
          </Group>
        );
      }
      return (
        <DatePickerInput
          size="xs"
          value={rule.value ? new Date(String(rule.value)) : null}
          onChange={(d: unknown) => onChange({ value: d ? (d instanceof Date ? d.toISOString() : String(d)) : undefined })}
          style={{ flex: 1, minWidth: 130 }}
          placeholder="pick date"
        />
      );

    case 'bool':
      return (
        <Switch
          size="xs"
          checked={Boolean(rule.value)}
          onChange={(e) => onChange({ value: e.currentTarget.checked })}
          label={rule.value ? 'Yes' : 'No'}
        />
      );

    case 'select':
      return <SelectValuePicker config={config} rule={rule} onChange={onChange} />;

    case 'color':
      return <ColorValuePicker rule={rule} onChange={onChange} />;

    default:
      return null;
  }
}

function TagValuePicker({ rule, onChange }: { rule: SmartRule; onChange: (p: Partial<SmartRule>) => void }) {
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLDivElement>(null);
  const selected = rule.values ?? [];

  return (
    <div style={{ flex: 1, display: 'flex', flexWrap: 'wrap', gap: 4, alignItems: 'center' }}>
      {selected.map((tag) => (
        <NamespaceTagChip
          key={tag}
          tag={tag}
          size="md"
          onRemove={() => onChange({ values: selected.filter((t) => t !== tag) })}
        />
      ))}
      <div ref={btnRef}>
        <ActionIcon size="xs" variant="subtle" color="dimmed" onClick={() => setOpen(true)}>
          <IconPlus size={12} />
        </ActionIcon>
      </div>
      {open && (
        <TagPickerMenu
          selected={selected}
          onChange={(tags) => onChange({ values: tags })}
          anchorRef={btnRef}
          onClose={() => setOpen(false)}
        />
      )}
    </div>
  );
}

function SelectValuePicker({ config, rule, onChange }: { config: FieldConfig; rule: SmartRule; onChange: (p: Partial<SmartRule>) => void }) {
  return (
    <Select
      size="xs"
      data={config.selectOptions ?? []}
      value={String(rule.value ?? '')}
      onChange={(v) => onChange({ value: v ?? '' })}
      style={{ flex: 1, minWidth: 120 }}
      placeholder="select..."
    />
  );
}

function ColorValuePicker({ rule, onChange }: { rule: SmartRule; onChange: (p: Partial<SmartRule>) => void }) {
  const selected = rule.values ?? [];

  const toggleColor = (hex: string) => {
    const set = new Set(selected);
    if (set.has(hex)) set.delete(hex);
    else set.add(hex);
    onChange({ values: Array.from(set) });
  };

  return (
    <div style={{ flex: 1 }}>
      <SimpleGrid cols={10} spacing={4}>
        {COLOR_SWATCHES.map((hex) => (
          <ColorSwatch
            key={hex}
            color={hex}
            size={20}
            style={{
              cursor: 'pointer',
              outline: selected.includes(hex) ? '2px solid var(--mantine-color-blue-5)' : 'none',
              outlineOffset: 1,
              borderRadius: 4,
            }}
            onClick={() => toggleColor(hex)}
          />
        ))}
      </SimpleGrid>
    </div>
  );
}

function FileSizeValuePicker({ rule, onChange, isBetween }: { rule: SmartRule; onChange: (p: Partial<SmartRule>) => void; isBetween: boolean }) {
  const [unit, setUnit] = useState('1048576'); // MB default

  const updateValue = (raw: number) => {
    onChange({ value: raw * Number(unit) });
  };
  const updateValue2 = (raw: number) => {
    onChange({ value2: raw * Number(unit) });
  };

  const displayVal = Number(rule.value ?? 0) / Number(unit);
  const displayVal2 = Number(rule.value2 ?? 0) / Number(unit);

  return (
    <Group gap={4} style={{ flex: 1 }}>
      <NumberInput
        size="xs"
        value={displayVal}
        onChange={(v) => updateValue(Number(v))}
        style={{ width: isBetween ? 70 : 90 }}
        min={0}
        decimalScale={1}
      />
      {isBetween && (
        <>
          <span style={{ fontSize: 'var(--mantine-font-size-xs)', color: 'var(--mantine-color-dimmed)' }}>and</span>
          <NumberInput
            size="xs"
            value={displayVal2}
            onChange={(v) => updateValue2(Number(v))}
            style={{ width: 70 }}
            min={0}
            decimalScale={1}
          />
        </>
      )}
      <Select
        size="xs"
        data={SIZE_UNITS}
        value={unit}
        onChange={(v) => setUnit(v ?? '1048576')}
        style={{ width: 65 }}
      />
    </Group>
  );
}

function DateDurationPicker({ rule, onChange }: { rule: SmartRule; onChange: (p: Partial<SmartRule>) => void }) {
  // Parse value like "7d" or "30d"
  const valStr = String(rule.value ?? '7d');
  const numPart = parseInt(valStr) || 7;
  const unitPart = valStr.replace(/\d+/g, '') || 'd';

  const update = (num: number, u: string) => {
    onChange({ value: `${num}${u}` });
  };

  return (
    <Group gap={4} style={{ flex: 1 }}>
      <NumberInput
        size="xs"
        value={numPart}
        onChange={(v) => update(Number(v) || 1, unitPart)}
        style={{ width: 70 }}
        min={1}
      />
      <Select
        size="xs"
        data={DATE_DURATION_UNITS}
        value={unitPart}
        onChange={(v) => update(numPart, v ?? 'd')}
        style={{ width: 90 }}
      />
    </Group>
  );
}

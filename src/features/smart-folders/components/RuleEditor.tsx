import { Group, ActionIcon, Select } from '@mantine/core';
import { IconMinus, IconPlus } from '@tabler/icons-react';
import { FIELD_CONFIGS, getFieldConfig } from './fieldConfig';
import { ValuePicker } from './ValuePicker';
import type { SmartRule } from './types';
import { createDefaultRule } from './types';

interface RuleEditorProps {
  rule: SmartRule;
  onChange: (rule: SmartRule) => void;
  onRemove: () => void;
  onAddRule: () => void;
  canRemove: boolean;
}

const fieldSelectData = FIELD_CONFIGS.map((f) => ({
  value: f.key,
  label: f.label,
}));

export function RuleEditor({ rule, onChange, onRemove, onAddRule, canRemove }: RuleEditorProps) {
  const config = getFieldConfig(rule.field);

  const handleFieldChange = (field: string | null) => {
    if (!field) return;
    const newConfig = getFieldConfig(field);
    if (!newConfig) return;
    const defaultOp = newConfig.operators[0]?.value ?? '';
    const newRule = createDefaultRule();
    newRule.field = field;
    newRule.op = defaultOp;
    onChange(newRule);
  };

  const handleOpChange = (op: string | null) => {
    if (!op) return;
    onChange({ ...rule, op });
  };

  const handleValueChange = (partial: Partial<SmartRule>) => {
    onChange({ ...rule, ...partial });
  };

  return (
    <Group
      gap={6}
      wrap="nowrap"
      align="center"
      style={{
        width: '100%',
        padding: '4px 0',
      }}
    >
      <Select
        size="xs"
        data={fieldSelectData}
        value={rule.field}
        onChange={handleFieldChange}
        style={{ width: 130 }}
        comboboxProps={{ withinPortal: true }}
      />

      {config && (
        <Select
          size="xs"
          data={config.operators}
          value={rule.op}
          onChange={handleOpChange}
          style={{ width: 140 }}
          comboboxProps={{ withinPortal: true }}
        />
      )}

      {config && (
        <ValuePicker config={config} rule={rule} onChange={handleValueChange} />
      )}

      <Group gap={4} wrap="nowrap" style={{ marginLeft: 'auto', flexShrink: 0 }}>
        {canRemove && (
          <ActionIcon size={20} variant="subtle" color="dimmed" onClick={onRemove}>
            <IconMinus size={12} />
          </ActionIcon>
        )}
        <ActionIcon size={20} variant="subtle" color="dimmed" onClick={onAddRule}>
          <IconPlus size={12} />
        </ActionIcon>
      </Group>
    </Group>
  );
}

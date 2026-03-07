import { Stack, Group, Select, ActionIcon, Text } from '@mantine/core';
import { IconMinus, IconPlus } from '@tabler/icons-react';
import { RuleEditor } from './RuleEditor';
import type { SmartRuleGroup, SmartRule } from './types';
import { createDefaultRule } from './types';

interface RuleGroupEditorProps {
  group: SmartRuleGroup;
  onChange: (group: SmartRuleGroup) => void;
  onRemove: () => void;
  onAddGroup: () => void;
  canRemove: boolean;
}

export function RuleGroupEditor({ group, onChange, onRemove, onAddGroup, canRemove }: RuleGroupEditorProps) {
  const handleRuleChange = (index: number, rule: SmartRule) => {
    const rules = [...group.rules];
    rules[index] = rule;
    onChange({ ...group, rules });
  };

  const handleRuleRemove = (index: number) => {
    const rules = group.rules.filter((_, i) => i !== index);
    onChange({ ...group, rules });
  };

  const handleAddRule = () => {
    onChange({ ...group, rules: [...group.rules, createDefaultRule()] });
  };

  return (
    <div
      style={{
        border: '1px solid var(--mantine-color-default-border)',
        borderRadius: 'var(--mantine-radius-sm)',
        padding: 'var(--mantine-spacing-xs)',
        background: group.negate
          ? 'var(--mantine-color-red-light)'
          : 'var(--mantine-color-body)',
      }}
    >
      <Stack gap={4}>
        {/* Sentence-style group header: "[any ▼] of the following are [true ▼]  [- +]" */}
        <Group gap={6} justify="space-between" wrap="nowrap">
          <Group gap={6} align="center" wrap="nowrap">
            <Select
              size="xs"
              data={[
                { value: 'any', label: 'any' },
                { value: 'all', label: 'all' },
              ]}
              value={group.match_mode}
              onChange={(v) => onChange({ ...group, match_mode: (v as 'all' | 'any') ?? 'all' })}
              style={{ width: 70 }}
              comboboxProps={{ withinPortal: true }}
              styles={{ input: { fontWeight: 500, fontSize: 'var(--mantine-font-size-xs)' } }}
            />
            <Text size="xs" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
              of the following are
            </Text>
            <Select
              size="xs"
              data={[
                { value: 'false', label: 'true' },
                { value: 'true', label: 'false' },
              ]}
              value={group.negate ? 'true' : 'false'}
              onChange={(v) => onChange({ ...group, negate: v === 'true' })}
              style={{ width: 75 }}
              comboboxProps={{ withinPortal: true }}
              styles={{ input: { fontWeight: 500, fontSize: 'var(--mantine-font-size-xs)' } }}
            />
          </Group>
          <Group gap={4} wrap="nowrap">
            {canRemove && (
              <ActionIcon size={20} variant="subtle" color="dimmed" onClick={onRemove}>
                <IconMinus size={12} />
              </ActionIcon>
            )}
            <ActionIcon size={20} variant="subtle" color="dimmed" onClick={onAddGroup}>
              <IconPlus size={12} />
            </ActionIcon>
          </Group>
        </Group>

        {/* Rule rows */}
        {group.rules.map((rule, i) => (
          <RuleEditor
            key={i}
            rule={rule}
            onChange={(r) => handleRuleChange(i, r)}
            onRemove={() => handleRuleRemove(i)}
            onAddRule={handleAddRule}
            canRemove={group.rules.length > 1}
          />
        ))}
      </Stack>
    </div>
  );
}

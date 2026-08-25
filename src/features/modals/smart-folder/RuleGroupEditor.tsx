/**
 * RuleGroupEditor — one predicate group with match mode, negate toggle, and rule list.
 * Sentence-style header: "Match [any/all] of the following [+group] [-group]"
 */

import { useCallback } from 'react';
import { IconTrash } from '@tabler/icons-react';
import { modalStyles } from '../../../shared/ui/GlassModal';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { ToggleSwitch } from '../../../shared/ui/ToggleSwitch/ToggleSwitch';
import type {
  SmartFolderPredicateGroup,
  SmartFolderPredicateRule,
} from '../../../shared/types/canonical';
import { RuleEditor } from './RuleEditor';
import { defaultOperator } from './fieldConfig';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';

export interface RuleGroupEditorProps {
  group: SmartFolderPredicateGroup;
  onChange: (group: SmartFolderPredicateGroup) => void;
  onRemove: () => void;
  canRemove: boolean;
}

const MATCH_MODE_OPTIONS = [
  { value: 'all', label: 'all' },
  { value: 'any', label: 'any' },
];

function makeDefaultRule(): SmartFolderPredicateRule {
  return { field: 'tags', op: defaultOperator('tags'), values: [] };
}

export function RuleGroupEditor({ group, onChange, onRemove, canRemove }: RuleGroupEditorProps) {
  const handleMatchModeChange = useCallback(
    (value: string) => onChange({ ...group, match_mode: value as 'all' | 'any' }),
    [group, onChange],
  );

  const handleNegateToggle = useCallback(
    () => onChange({ ...group, negate: !group.negate }),
    [group, onChange],
  );

  const handleRuleChange = useCallback(
    (index: number, rule: SmartFolderPredicateRule) => {
      const nextRules = group.rules.map((r, i) => (i === index ? rule : r));
      onChange({ ...group, rules: nextRules });
    },
    [group, onChange],
  );

  const handleRuleAdd = useCallback(
    (afterIndex: number) => {
      const nextRules = [...group.rules];
      nextRules.splice(afterIndex + 1, 0, makeDefaultRule());
      onChange({ ...group, rules: nextRules });
    },
    [group, onChange],
  );

  const handleRuleRemove = useCallback(
    (index: number) => {
      const nextRules = group.rules.filter((_, i) => i !== index);
      onChange({ ...group, rules: nextRules });
    },
    [group, onChange],
  );

  const containerClass = group.negate
    ? `${modalStyles.group} ${modalStyles.groupNegated}`
    : modalStyles.group;

  return (
    <div className={containerClass}>
      {/* Header row: sentence-style match mode + negate + remove */}
      <div className={modalStyles.row}>
        <span className={modalStyles.inlineLabel}>Match</span>
        <CmSelect
          value={group.match_mode}
          options={MATCH_MODE_OPTIONS}
          onChange={handleMatchModeChange}
          width={72}
        />
        <span className={modalStyles.inlineLabel}>of the following</span>
        <div style={{ flex: 1 }} />
        <span className={modalStyles.inlineLabel}>Negate</span>
        <ToggleSwitch on={!!group.negate} onChange={handleNegateToggle} />
        {canRemove && (
          <KbdTooltip label="Remove group"><button
            className={`${modalStyles.actionBtn} ${modalStyles.actionBtnDanger}`}
            onClick={onRemove}
            type="button"
            aria-label="Remove group"
          >
            <IconTrash size={14} />
          </button></KbdTooltip>
        )}
      </div>

      {/* Rule list */}
      <div className={modalStyles.stackSm}>
        {group.rules.map((rule, index) => (
          <RuleEditor
            key={index}
            rule={rule}
            onChange={(next) => handleRuleChange(index, next)}
            onRemove={() => handleRuleRemove(index)}
            onAdd={() => handleRuleAdd(index)}
            canRemove={group.rules.length > 1}
          />
        ))}
      </div>
    </div>
  );
}

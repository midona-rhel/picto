/**
 * RuleGroupEditor — one predicate group with match mode, negate toggle, and rule list.
 * Sentence-style header: "Match [any/all] of the following [+group] [-group]"
 */

import { useCallback } from 'react';
import { modalStyles } from '../../../shared/ui/GlassModal';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import type {
  SmartFolderPredicateGroup,
  SmartFolderPredicateRule,
} from '../../../shared/types/canonical';
import { RuleEditor } from './RuleEditor';
import { defaultOperator } from './fieldConfig';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import styles from '../SmartFolderModal.module.css';

export interface RuleGroupEditorProps {
  group: SmartFolderPredicateGroup;
  onChange: (group: SmartFolderPredicateGroup) => void;
  onRemove: () => void;
  onAdd: () => void;
  canRemove: boolean;
}

const MATCH_MODE_OPTIONS = [
  { value: 'all', label: 'all' },
  { value: 'any', label: 'any' },
];

function makeDefaultRule(): SmartFolderPredicateRule {
  return { field: 'tags', op: defaultOperator('tags'), values: [] };
}

export function RuleGroupEditor({ group, onChange, onRemove, onAdd, canRemove }: RuleGroupEditorProps) {
  const handleMatchModeChange = useCallback(
    (value: string) => onChange({ ...group, match_mode: value as 'all' | 'any' }),
    [group, onChange],
  );

  const handleNegateChange = useCallback(
    (value: string) => onChange({ ...group, negate: value === 'exclude' }),
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

  return (
    <div className={styles.condition}>
      {/* Header row: sentence-style match mode + negate + remove */}
      <div className={styles.conditionHeader}>
        <span className={modalStyles.inlineLabel}>Match</span>
        <CmSelect
          value={group.match_mode}
          options={MATCH_MODE_OPTIONS}
          onChange={handleMatchModeChange}
          width={72}
        />
        <span className={modalStyles.inlineLabel}>of the following</span>
        <CmSelect
          value={group.negate ? 'exclude' : 'include'}
          options={[{ value: 'include', label: 'included' }, { value: 'exclude', label: 'excluded' }]}
          onChange={handleNegateChange}
          width={92}
        />
        <div className={styles.conditionActions}>
          <KbdTooltip label="Remove group"><button
            className={styles.conditionButton}
            onClick={onRemove}
            type="button"
            aria-label="Remove group"
            disabled={!canRemove}
          >
            <span className={styles.conditionGlyph} aria-hidden="true" />
          </button></KbdTooltip>
          <KbdTooltip label="Add group"><button
            className={styles.conditionButton}
            onClick={onAdd}
            type="button"
            aria-label="Add group"
          >
            <span className={`${styles.conditionGlyph} ${styles.conditionGlyphPlus}`} aria-hidden="true" />
          </button></KbdTooltip>
        </div>
      </div>

      {/* Rule list */}
      <div className={styles.rules}>
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

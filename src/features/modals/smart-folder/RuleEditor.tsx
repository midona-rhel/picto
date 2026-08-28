/**
 * RuleEditor — one rule row inside a predicate group.
 * Layout: [Field CmSelect] [Op CmSelect] [Value input(s)] [-] [+]
 */

import { modalStyles } from '../../../shared/ui/GlassModal';
import { GlassInput } from '../../../shared/ui/GlassInput';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { TagTokenInput } from '../../../shared/ui/TagTokenInput';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import type { SmartFolderPredicateRule } from '../../../shared/types/canonical';
import {
  getFieldDef,
  getFieldOptions,
  defaultOperator,
  defaultValue,
  isListField,
  FILESIZE_UNITS,
} from './fieldConfig';
import styles from '../SmartFolderModal.module.css';

export interface RuleEditorProps {
  rule: SmartFolderPredicateRule;
  onChange: (rule: SmartFolderPredicateRule) => void;
  onRemove: () => void;
  onAdd: () => void;
  canRemove: boolean;
}

function valuesText(rule: SmartFolderPredicateRule): string {
  return (rule.values ?? []).join(', ');
}

function parseCsvValues(raw: string): string[] {
  return raw
    .split(',')
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
}

export function RuleEditor({ rule, onChange, onRemove, onAdd, canRemove }: RuleEditorProps) {
  const fieldDef = getFieldDef(rule.field);
  const isBetween = rule.op === 'between';

  const handleFieldChange = (nextField: string) => {
    const nextList = isListField(nextField);
    onChange({
      field: nextField,
      op: defaultOperator(nextField),
      value: nextList ? undefined : defaultValue(nextField),
      value2: undefined,
      values: nextList ? [] : undefined,
    });
  };

  const handleOpChange = (nextOp: string) => {
    onChange({ ...rule, op: nextOp, value2: nextOp === 'between' ? rule.value2 ?? rule.value : undefined });
  };

  const renderValueInput = () => {
    switch (fieldDef.valueType) {
      case 'tags':
        return (
          <TagTokenInput
            values={rule.values ?? []}
            onChange={(values) => onChange({ ...rule, values })}
            compact
          />
        );

      case 'color':
        return (
          <GlassInput
            value={valuesText(rule)}
            onChange={(e) => onChange({ ...rule, values: parseCsvValues(e.target.value) })}
            placeholder="#ff0000, #00ff00"
          />
        );

      case 'select':
        return (
          <CmSelect
            value={typeof rule.value === 'string' ? rule.value : (fieldDef.selectOptions?.[0]?.value ?? '')}
            options={fieldDef.selectOptions ?? []}
            onChange={(v) => onChange({ ...rule, value: v })}
          />
        );

      case 'filesize':
        return (
          <div className={modalStyles.row}>
            <GlassInput
              type="number"
              value={rule.value == null ? '' : String(rule.value)}
              onChange={(e) => onChange({ ...rule, value: Number(e.target.value || 0) })}
              placeholder="Size"
              style={{ flex: 1 }}
            />
            {isBetween && (
              <>
                <span className={modalStyles.inlineLabel}>and</span>
                <GlassInput
                  type="number"
                  value={rule.value2 == null ? '' : String(rule.value2)}
                  onChange={(e) => onChange({ ...rule, value2: Number(e.target.value || 0) })}
                  placeholder="Max"
                  style={{ flex: 1 }}
                />
              </>
            )}
            <CmSelect
              value="MB"
              options={FILESIZE_UNITS}
              onChange={() => {}}
              width={64}
            />
          </div>
        );

      case 'number':
        return (
          <div className={modalStyles.row} style={{ flex: 1 }}>
            <GlassInput
              type="number"
              value={rule.value == null ? '' : String(rule.value)}
              onChange={(e) => onChange({ ...rule, value: Number(e.target.value || 0) })}
              placeholder="Value"
              style={{ flex: 1 }}
            />
            {isBetween && (
              <>
                <span className={modalStyles.inlineLabel}>and</span>
                <GlassInput
                  type="number"
                  value={rule.value2 == null ? '' : String(rule.value2)}
                  onChange={(e) => onChange({ ...rule, value2: Number(e.target.value || 0) })}
                  placeholder="Max"
                  style={{ flex: 1 }}
                />
              </>
            )}
            {fieldDef.unit && (
              <span className={modalStyles.inlineLabel}>{fieldDef.unit}</span>
            )}
          </div>
        );

      case 'date':
        return (
          <div className={modalStyles.row} style={{ flex: 1 }}>
            <GlassInput
              type="date"
              value={rule.value == null ? '' : String(rule.value)}
              onChange={(e) => onChange({ ...rule, value: e.target.value })}
              style={{ flex: 1 }}
            />
            {isBetween && (
              <>
                <span className={modalStyles.inlineLabel}>and</span>
                <GlassInput
                  type="date"
                  value={rule.value2 == null ? '' : String(rule.value2)}
                  onChange={(e) => onChange({ ...rule, value2: e.target.value })}
                  style={{ flex: 1 }}
                />
              </>
            )}
          </div>
        );

      case 'text':
        return (
          <GlassInput
            value={rule.value == null ? '' : String(rule.value)}
            onChange={(e) => onChange({ ...rule, value: e.target.value })}
            placeholder="Value"
          />
        );
    }
  };

  const showValue = !['is_empty', 'is_not_empty'].includes(rule.op);

  return (
    <div className={styles.rule}>
      <CmSelect
        value={rule.field}
        options={getFieldOptions()}
        onChange={handleFieldChange}
        width={130}
      />
      <CmSelect
        value={rule.op}
        options={fieldDef.operators}
        onChange={handleOpChange}
        width={120}
      />
      {showValue && (
        <div className={styles.ruleValue}>
          {renderValueInput()}
        </div>
      )}
      <KbdTooltip label="Remove rule"><button
        className={styles.conditionButton}
        onClick={onRemove}
        type="button"
        aria-label="Remove rule"
        disabled={!canRemove}
      >
        <span className={styles.conditionGlyph} aria-hidden="true" />
      </button></KbdTooltip>
      <KbdTooltip label="Add rule"><button
        className={styles.conditionButton}
        onClick={onAdd}
        type="button"
        aria-label="Add rule"
      >
        <span className={`${styles.conditionGlyph} ${styles.conditionGlyphPlus}`} aria-hidden="true" />
      </button></KbdTooltip>
    </div>
  );
}

/**
 * RuleEditor — one rule row inside a predicate group.
 * Layout: [Field CmSelect] [Op CmSelect] [Value input(s)] [-] [+]
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { modalStyles } from '../../../shared/ui/GlassModal';
import { GlassInput } from '../../../shared/ui/GlassInput';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { TagTokenInput } from '../../../shared/ui/TagTokenInput';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import { ColorFilterEditor } from '../../../shared/ui/ColorFilterEditor';
import { DatePickerButton } from '../../../shared/ui/DatePickerButton';
import type { SmartFolderPredicateRule } from '../../../shared/types/canonical';
import { listAcceptedMediaFormats } from '../../../platform/mediaFormatApi';
import {
  getFieldDef,
  getFieldOptions,
  defaultOperator,
  defaultValue,
  isListField,
  FILESIZE_UNITS,
  RATING_OPTIONS,
} from './fieldConfig';
import styles from '../SmartFolderModal.module.css';

export interface RuleEditorProps {
  rule: SmartFolderPredicateRule;
  onChange: (rule: SmartFolderPredicateRule) => void;
  onRemove: () => void;
  onAdd: () => void;
  canRemove: boolean;
  canAdd: boolean;
}

function ColorRuleInput({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 0, top: 0 });

  const show = () => {
    const rect = triggerRef.current?.getBoundingClientRect();
    if (rect) {
      const width = 230;
      const height = 286;
      setPosition({
        left: Math.max(8, Math.min(rect.left, window.innerWidth - width - 8)),
        top: rect.bottom + height <= window.innerHeight - 8
          ? rect.bottom + 5
          : Math.max(8, rect.top - height - 5),
      });
    }
    setOpen(true);
  };

  return (
    <>
      <button ref={triggerRef} type="button" className={styles.colorTrigger} onClick={() => (open ? setOpen(false) : show())}>
        <span className={styles.colorSwatch} style={{ background: value }} aria-hidden="true" />
        <span className={styles.colorValue}>{value.toUpperCase()}</span>
      </button>
      {open && createPortal(
        <>
          <button className={styles.colorPickerBackdrop} type="button" aria-label="Close color picker" onClick={() => setOpen(false)} />
          <div className={styles.colorPickerPopover} style={position} role="dialog" aria-label="Choose color">
            <ColorFilterEditor value={value} allowClear={false} onCommit={(next) => { if (next) onChange(next); }} />
          </div>
        </>,
        document.body,
      )}
    </>
  );
}

export function RuleEditor({ rule, onChange, onRemove, onAdd, canRemove, canAdd }: RuleEditorProps) {
  const fieldDef = getFieldDef(rule.field);
  const isBetween = rule.op === 'between';
  const [acceptedFormats, setAcceptedFormats] = useState<{ value: string; label: string }[]>([]);

  useEffect(() => {
    if (rule.field !== 'file_type' || acceptedFormats.length > 0) return;
    let cancelled = false;
    void listAcceptedMediaFormats()
      .then((formats) => {
        if (cancelled) return;
        const extensions = new Map<string, Set<string>>();
        for (const format of formats) {
          const known = extensions.get(format.mime_type) ?? new Set<string>();
          known.add(format.extension.toUpperCase());
          extensions.set(format.mime_type, known);
        }
        const families = [
          ['image', 'All Images'], ['video', 'All Videos'], ['audio', 'All Audio'],
          ['model', 'All 3D Models'], ['font', 'All Fonts'], ['text', 'All Text'],
          ['application', 'All Documents and Project Files'],
        ].map(([value, label]) => ({ value, label }));
        const exact = [...extensions.entries()]
          .map(([value, values]) => ({
            value,
            label: [...values].slice(0, 4).join(' / '),
          }))
          .sort((left, right) => left.label.localeCompare(right.label));
        setAcceptedFormats([...families, ...exact]);
      })
      .catch(() => { /* The built-in family options remain available. */ });
    return () => { cancelled = true; };
  }, [acceptedFormats.length, rule.field]);

  const selectOptions = useMemo(() => (
    rule.field === 'file_type' && acceptedFormats.length > 0
      ? acceptedFormats
      : fieldDef.selectOptions ?? []
  ), [acceptedFormats, fieldDef.selectOptions, rule.field]);

  const handleFieldChange = (nextField: string) => {
    const nextList = isListField(nextField);
    onChange({
      field: nextField,
      op: defaultOperator(nextField),
      value: nextList ? undefined : defaultValue(nextField),
      value2: undefined,
      values: nextList ? [] : undefined,
      unit: nextField === 'file_size' ? 'MB' : undefined,
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
          <ColorRuleInput
            value={rule.values?.[0] ?? '#808080'}
            onChange={(color) => onChange({ ...rule, values: [color] })}
          />
        );

      case 'select':
        return (
          <CmSelect
            value={typeof rule.value === 'string' ? rule.value : (selectOptions[0]?.value ?? '')}
            options={selectOptions}
            onChange={(v) => onChange({ ...rule, value: v })}
            width={180}
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
              value={rule.unit ?? 'MB'}
              options={FILESIZE_UNITS}
              onChange={(unit) => onChange({ ...rule, unit })}
              width={116}
            />
          </div>
        );

      case 'number':
        if (rule.field === 'rating') {
          const ratingSelect = (value: unknown, key: 'value' | 'value2') => (
            <CmSelect
              value={String(value ?? 0)}
              options={RATING_OPTIONS}
              onChange={(next) => onChange({ ...rule, [key]: Number(next) })}
              width={112}
              ariaLabel={key === 'value' ? 'Rating' : 'Maximum rating'}
            />
          );
          return (
            <div className={modalStyles.row} style={{ flex: 1 }}>
              {ratingSelect(rule.value, 'value')}
              {isBetween && (
                <>
                  <span className={modalStyles.inlineLabel}>and</span>
                  {ratingSelect(rule.value2 ?? rule.value, 'value2')}
                </>
              )}
            </div>
          );
        }
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
            <DatePickerButton
              value={rule.value == null ? '' : String(rule.value)}
              onChange={(value) => onChange({ ...rule, value })}
              ariaLabel="Choose date"
            />
            {isBetween && (
              <>
                <span className={modalStyles.inlineLabel}>and</span>
                <DatePickerButton
                  value={rule.value2 == null ? '' : String(rule.value2)}
                  onChange={(value) => onChange({ ...rule, value2: value })}
                  ariaLabel="Choose end date"
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
        width={140}
      />
      <CmSelect
        value={rule.op}
        options={fieldDef.operators}
        onChange={handleOpChange}
        width={128}
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
      <KbdTooltip label={canAdd ? 'Add rule' : 'Maximum 10 rules'}><button
        className={styles.conditionButton}
        onClick={onAdd}
        type="button"
        aria-label="Add rule"
        disabled={!canAdd}
      >
        <span className={`${styles.conditionGlyph} ${styles.conditionGlyphPlus}`} aria-hidden="true" />
      </button></KbdTooltip>
    </div>
  );
}

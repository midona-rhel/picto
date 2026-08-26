/**
 * RuleEditor — one rule row inside a predicate group.
 * Layout: [Field CmSelect] [Op CmSelect] [Value input(s)] [-] [+]
 */

import { useState, useRef, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { modalStyles } from '../../../shared/ui/GlassModal';
import { GlassInput } from '../../../shared/ui/GlassInput';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { ToggleSwitch } from '../../../shared/ui/ToggleSwitch/ToggleSwitch';
import { TagChip } from '../../../shared/ui/TagChip/TagChip';
import { tagsController } from '../../../controllers/tagsController';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import type { SmartFolderPredicateRule, CanonicalTagRecord } from '../../../shared/types/canonical';
import {
  getFieldDef,
  getFieldOptions,
  defaultOperator,
  defaultValue,
  isListField,
  FILESIZE_UNITS,
} from './fieldConfig';
import styles from '../SmartFolderModal.module.css';

// ── Tag input with chips + autocomplete ──

function parseTag(full: string): { namespace: string; subtag: string } {
  const idx = full.indexOf(':');
  if (idx < 0) return { namespace: 'general', subtag: full };
  return { namespace: full.slice(0, idx), subtag: full.slice(idx + 1) };
}

function formatTag(r: CanonicalTagRecord): string {
  if (!r.namespace || r.namespace === 'general' || r.namespace === '') return r.subtag;
  return `${r.namespace}:${r.subtag}`;
}

function TagAutoInput({ values, onChange }: { values: string[]; onChange: (v: string[]) => void }) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<CanonicalTagRecord[]>([]);
  const [showDrop, setShowDrop] = useState(false);
  const [selIdx, setSelIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const doSearch = (q: string) => {
    if (timerRef.current) clearTimeout(timerRef.current);
    if (q.length < 1) { setResults([]); setShowDrop(false); return; }
    timerRef.current = setTimeout(() => {
      void tagsController.getPaginated({ search: q, limit: 12 }).then(({ items }) => {
        const existing = new Set(values);
        const filtered = items.filter((r) => !existing.has(formatTag(r)));
        setResults(filtered.slice(0, 8));
        setShowDrop(filtered.length > 0);
        setSelIdx(0);
      }).catch(() => {});
    }, 120);
  };

  const addTag = (tag: string) => {
    if (!values.includes(tag)) onChange([...values, tag]);
    setQuery('');
    setResults([]);
    setShowDrop(false);
    inputRef.current?.focus();
  };

  const removeTag = (tag: string) => {
    onChange(values.filter((v) => v !== tag));
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Backspace' && query === '' && values.length > 0) {
      removeTag(values[values.length - 1]);
      return;
    }
    if (showDrop && results.length > 0) {
      if (e.key === 'ArrowDown') { e.preventDefault(); setSelIdx((i) => Math.min(i + 1, results.length - 1)); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); setSelIdx((i) => Math.max(i - 1, 0)); }
      else if (e.key === 'Enter' || e.key === 'Tab') { e.preventDefault(); addTag(formatTag(results[selIdx])); }
      else if (e.key === 'Escape') { setShowDrop(false); }
    } else if (e.key === 'Enter' && query.trim()) {
      e.preventDefault();
      addTag(query.trim());
    }
  };

  useEffect(() => () => { if (timerRef.current) clearTimeout(timerRef.current); }, []);

  const rect = wrapRef.current?.getBoundingClientRect();

  return (
    <div ref={wrapRef} style={{ position: 'relative', flex: 1 }}>
      <div className="smartfolder-tag-input" style={{
        display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 4,
        padding: '3px 6px', minHeight: 32,
        border: '1px solid var(--color-border-primary)',
        borderRadius: 'var(--radius-sm)',
        background: 'var(--color-control-surface)',
      }}>
        {values.map((tag) => {
          const { namespace, subtag } = parseTag(tag);
          return <TagChip key={tag} namespace={namespace} subtag={subtag} onRemove={() => removeTag(tag)} />;
        })}
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => { setQuery(e.target.value); doSearch(e.target.value); }}
          onFocus={() => { if (results.length > 0) setShowDrop(true); }}
          onBlur={() => setTimeout(() => setShowDrop(false), 150)}
          onKeyDown={handleKeyDown}
          placeholder={values.length === 0 ? 'Search tags...' : ''}
          style={{
            flex: 1, minWidth: 60, border: 'none', outline: 'none',
            background: 'transparent', color: 'var(--color-text-primary)',
            fontSize: 'var(--font-size-md)', fontFamily: 'var(--font-family)',
            height: 24, padding: 0,
          }}
        />
      </div>
      {showDrop && rect && createPortal(
        <div style={{
          position: 'fixed', top: rect.bottom + 2, left: rect.left, width: rect.width,
          maxHeight: 200, overflowY: 'auto', scrollbarGutter: 'stable', zIndex: 10001,
          background: 'var(--glass-bg)', backdropFilter: 'var(--glass-blur)',
          border: '1px solid var(--color-border-secondary)',
          borderRadius: 'var(--radius-sm)', boxShadow: 'var(--shadow-panel)',
          padding: '4px 0',
        }}>
          {results.map((r, i) => (
            <div
              key={`${r.namespace}:${r.subtag}`}
              onMouseDown={(e) => { e.preventDefault(); addTag(formatTag(r)); }}
              style={{
                padding: '4px 8px', cursor: 'pointer',
                background: i === selIdx ? 'var(--color-surface-hover)' : 'transparent',
              }}
            >
              <TagChip namespace={r.namespace} subtag={r.subtag} />
            </div>
          ))}
        </div>,
        document.body,
      )}
    </div>
  );
}

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
          <TagAutoInput
            values={rule.values ?? []}
            onChange={(values) => onChange({ ...rule, values })}
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

      case 'boolean':
        return (
          <div className={modalStyles.row}>
            <ToggleSwitch
              on={rule.value !== false}
              onChange={() => onChange({ ...rule, value: !rule.value })}
            />
            <span className={modalStyles.inlineLabel}>
              {rule.value !== false ? 'Yes' : 'No'}
            </span>
          </div>
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

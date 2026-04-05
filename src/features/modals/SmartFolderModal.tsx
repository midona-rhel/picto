/**
 * SmartFolderModal — create or edit a smart folder.
 * Uses the canonical SmartFolderPredicate shape end to end.
 */

import { useState, useEffect, useCallback } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';
import { ColorPicker } from '../../shared/ui/ColorPicker';
import { IconPicker } from '../../shared/ui/IconPicker';
import type {
  SmartFolderCommandPayload,
  SmartFolderPredicate,
  SmartFolderPredicateGroup,
  SmartFolderPredicateRule,
} from '../../shared/types/canonical';

const FIELD_OPTIONS = [
  'tags',
  'rating',
  'date_added',
  'date_created',
  'date_modified',
  'file_size',
  'width',
  'height',
  'duration',
  'aspect_ratio',
  'name',
  'notes',
  'source_url',
  'file_type',
  'has_audio',
  'shape',
  'color',
] as const;

type SmartFolderField = typeof FIELD_OPTIONS[number];

function defaultRule(field: SmartFolderField = 'tags'): SmartFolderPredicateRule {
  return field === 'tags' || field === 'color'
    ? { field, op: field === 'color' ? 'contains' : 'include_any', values: [] }
    : { field, op: defaultOpForField(field), value: defaultValueForField(field) };
}

function defaultGroup(): SmartFolderPredicateGroup {
  return { match_mode: 'all', negate: false, rules: [defaultRule()] };
}

function emptyPredicate(): SmartFolderPredicate {
  return { groups: [defaultGroup()] };
}

function defaultOpForField(field: SmartFolderField): string {
  switch (field) {
    case 'tags': return 'include_any';
    case 'color': return 'contains';
    case 'rating':
    case 'file_size':
    case 'width':
    case 'height':
    case 'duration':
    case 'aspect_ratio':
      return 'gte';
    case 'date_added':
    case 'date_created':
    case 'date_modified':
      return 'gte';
    case 'file_type':
    case 'shape':
    case 'has_audio':
      return 'is';
    default:
      return 'contains';
  }
}

function defaultValueForField(field: SmartFolderField): unknown {
  switch (field) {
    case 'rating':
    case 'file_size':
    case 'width':
    case 'height':
    case 'duration':
    case 'aspect_ratio':
      return 0;
    case 'has_audio':
      return true;
    case 'shape':
      return 'landscape';
    case 'file_type':
      return 'image';
    case 'date_added':
    case 'date_created':
    case 'date_modified':
      return new Date().toISOString().slice(0, 10);
    default:
      return '';
  }
}

function normalizePredicate(input: SmartFolderPredicate | undefined): SmartFolderPredicate {
  if (!input?.groups?.length) return emptyPredicate();
  return {
    groups: input.groups.map((group) => ({
      match_mode: group.match_mode === 'any' ? 'any' : 'all',
      negate: !!group.negate,
      rules: group.rules?.length
        ? group.rules.map((rule) => normalizeRule(rule))
        : [defaultRule()],
    })),
  };
}

function normalizeRule(rule: SmartFolderPredicateRule): SmartFolderPredicateRule {
  const field = (FIELD_OPTIONS.includes(rule.field as SmartFolderField) ? rule.field : 'tags') as SmartFolderField;
  return {
    field,
    op: rule.op || defaultOpForField(field),
    value: rule.value ?? defaultValueForField(field),
    value2: rule.value2 ?? undefined,
    values: Array.isArray(rule.values) ? rule.values : (field === 'tags' || field === 'color' ? [] : undefined),
  };
}

function ruleOperatorOptions(field: SmartFolderField): string[] {
  switch (field) {
    case 'tags':
      return ['include_all', 'include_any', 'do_not_include'];
    case 'color':
      return ['contains'];
    case 'rating':
    case 'file_size':
    case 'width':
    case 'height':
    case 'duration':
    case 'aspect_ratio':
      return ['gte', 'lte', 'gt', 'lt', 'eq', 'between'];
    case 'date_added':
    case 'date_created':
    case 'date_modified':
      return ['gte', 'lte', 'gt', 'lt', 'eq'];
    case 'file_type':
      return ['is', 'is_not'];
    case 'has_audio':
      return ['is'];
    case 'shape':
      return ['is'];
    default:
      return ['contains', 'does_not_contain', 'is', 'is_not', 'starts_with', 'ends_with', 'is_empty', 'is_not_empty'];
  }
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

function buildPayload(data: {
  id?: number;
  name: string;
  parent_id?: number | null;
  icon: string | null;
  color: string | null;
  notes: string | null;
  predicate: SmartFolderPredicate;
  sort_field?: string | null;
  sort_order?: string | null;
  display_order?: number | null;
}): SmartFolderCommandPayload {
  return {
    smart_folder_id: data.id ?? 0,
    name: data.name,
    parent_id: data.parent_id ?? null,
    icon: data.icon,
    color: data.color,
    notes: data.notes,
    predicate_json: JSON.stringify(data.predicate),
    sort_field: data.sort_field ?? null,
    sort_order: data.sort_order ?? null,
    display_order: data.display_order ?? null,
    created_at: null,
    updated_at: null,
  };
}

export interface SmartFolderModalProps {
  open: boolean;
  onClose: () => void;
  onSave: (data: SmartFolderCommandPayload) => void;
  initial?: {
    id?: number;
    name?: string;
    parent_id?: number | null;
    icon?: string | null;
    color?: string | null;
    notes?: string | null;
    predicate?: SmartFolderPredicate;
    sort_field?: string | null;
    sort_order?: string | null;
    display_order?: number | null;
  };
  mode?: 'create' | 'edit';
}

export function SmartFolderModal({
  open, onClose, onSave, initial, mode = 'create',
}: SmartFolderModalProps) {
  const [name, setName] = useState('');
  const [icon, setIcon] = useState<string | null>(null);
  const [color, setColor] = useState<string | null>(null);
  const [notes, setNotes] = useState('');
  const [predicate, setPredicate] = useState<SmartFolderPredicate>(emptyPredicate());

  useEffect(() => {
    if (!open) return;
    setName(initial?.name ?? '');
    setIcon(initial?.icon ?? null);
    setColor(initial?.color ?? null);
    setNotes(initial?.notes ?? '');
    setPredicate(normalizePredicate(initial?.predicate));
  }, [open, initial]);

  const handleSave = useCallback(() => {
    if (!name.trim()) return;
    onSave(buildPayload({
      id: initial?.id,
      name: name.trim(),
      parent_id: initial?.parent_id ?? null,
      icon,
      color,
      notes: notes.trim() ? notes.trim() : null,
      predicate,
      sort_field: initial?.sort_field ?? null,
      sort_order: initial?.sort_order ?? null,
      display_order: initial?.display_order ?? null,
    }));
  }, [color, icon, initial, name, notes, onSave, predicate]);

  const updateGroup = useCallback((groupIndex: number, patch: Partial<SmartFolderPredicateGroup>) => {
    setPredicate((current) => ({
      groups: current.groups.map((group, index) => index === groupIndex ? { ...group, ...patch } : group),
    }));
  }, []);

  const removeGroup = useCallback((groupIndex: number) => {
    setPredicate((current) => ({
      groups: current.groups.filter((_, index) => index !== groupIndex),
    }));
  }, []);

  const addGroup = useCallback(() => {
    setPredicate((current) => ({ groups: [...current.groups, defaultGroup()] }));
  }, []);

  const updateRule = useCallback((groupIndex: number, ruleIndex: number, next: SmartFolderPredicateRule) => {
    setPredicate((current) => ({
      groups: current.groups.map((group, index) => {
        if (index !== groupIndex) return group;
        return {
          ...group,
          rules: group.rules.map((rule, innerIndex) => innerIndex === ruleIndex ? next : rule),
        };
      }),
    }));
  }, []);

  const addRule = useCallback((groupIndex: number) => {
    setPredicate((current) => ({
      groups: current.groups.map((group, index) => (
        index === groupIndex
          ? { ...group, rules: [...group.rules, defaultRule()] }
          : group
      )),
    }));
  }, []);

  const removeRule = useCallback((groupIndex: number, ruleIndex: number) => {
    setPredicate((current) => ({
      groups: current.groups.map((group, index) => {
        if (index !== groupIndex) return group;
        return {
          ...group,
          rules: group.rules.filter((_, innerIndex) => innerIndex !== ruleIndex),
        };
      }),
    }));
  }, []);

  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title={mode === 'create' ? 'New Smart Folder' : 'Edit Smart Folder'}
      size="lg"
      footer={(
        <>
          <button className={modalStyles.btn} onClick={onClose} type="button">Cancel</button>
          <button
            className={`${modalStyles.btn} ${modalStyles.btnPrimary}`}
            onClick={handleSave}
            disabled={!name.trim()}
            type="button"
          >
            {mode === 'create' ? 'Create' : 'Save'}
          </button>
        </>
      )}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Name</label>
          <GlassInput
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Smart folder name"
            autoFocus
          />
        </div>

        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Notes</label>
          <textarea
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            placeholder="Notes"
            style={{ minHeight: 72, resize: 'vertical' }}
          />
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16 }}>
          <div className={modalStyles.field}>
            <label className={modalStyles.fieldLabel}>Icon</label>
            <IconPicker value={icon} onChange={setIcon} />
          </div>

          <div className={modalStyles.field}>
            <label className={modalStyles.fieldLabel}>Color</label>
            <ColorPicker value={color} onChange={setColor} />
          </div>
        </div>

        <div className={modalStyles.separator} />

        <div className={modalStyles.field}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <label className={modalStyles.fieldLabel}>Rules</label>
            <button className={modalStyles.btn} type="button" onClick={addGroup}>Add Group</button>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            {predicate.groups.map((group, groupIndex) => (
              <div key={`group-${groupIndex}`} style={{ border: '1px solid rgba(255,255,255,0.12)', borderRadius: 12, padding: 12, display: 'flex', flexDirection: 'column', gap: 12 }}>
                <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
                  <label className={modalStyles.fieldLabel}>Group</label>
                  <select
                    value={group.match_mode}
                    onChange={(e) => updateGroup(groupIndex, { match_mode: e.target.value as 'all' | 'any' })}
                  >
                    <option value="all">Match all rules</option>
                    <option value="any">Match any rule</option>
                  </select>
                  <label style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                    <input
                      type="checkbox"
                      checked={!!group.negate}
                      onChange={(e) => updateGroup(groupIndex, { negate: e.target.checked })}
                    />
                    Negate group
                  </label>
                  {predicate.groups.length > 1 && (
                    <button className={modalStyles.btn} type="button" onClick={() => removeGroup(groupIndex)}>Remove Group</button>
                  )}
                </div>

                {group.rules.map((rule, ruleIndex) => {
                  const field = (FIELD_OPTIONS.includes(rule.field as SmartFolderField) ? rule.field : 'tags') as SmartFolderField;
                  const operators = ruleOperatorOptions(field);
                  const isListRule = field === 'tags' || field === 'color';
                  const isBetween = rule.op === 'between';
                  const isBoolean = field === 'has_audio';
                  const isEnum = field === 'shape' || field === 'file_type';
                  const isDate = field === 'date_added' || field === 'date_created' || field === 'date_modified';
                  const isNumeric = ['rating', 'file_size', 'width', 'height', 'duration', 'aspect_ratio'].includes(field);

                  return (
                    <div key={`rule-${groupIndex}-${ruleIndex}`} style={{ borderTop: '1px solid rgba(255,255,255,0.08)', paddingTop: 12, display: 'grid', gap: 8 }}>
                      <div style={{ display: 'grid', gridTemplateColumns: '1.2fr 1fr auto', gap: 8, alignItems: 'center' }}>
                        <select
                          value={field}
                          onChange={(e) => {
                            const nextField = e.target.value as SmartFolderField;
                            updateRule(groupIndex, ruleIndex, defaultRule(nextField));
                          }}
                        >
                          {FIELD_OPTIONS.map((option) => (
                            <option key={option} value={option}>{option.replace(/_/g, ' ')}</option>
                          ))}
                        </select>
                        <select
                          value={rule.op}
                          onChange={(e) => updateRule(groupIndex, ruleIndex, { ...rule, op: e.target.value })}
                        >
                          {operators.map((option) => (
                            <option key={option} value={option}>{option.replace(/_/g, ' ')}</option>
                          ))}
                        </select>
                        {group.rules.length > 1 && (
                          <button className={modalStyles.btn} type="button" onClick={() => removeRule(groupIndex, ruleIndex)}>Remove</button>
                        )}
                      </div>

                      {isListRule ? (
                        <GlassInput
                          value={valuesText(rule)}
                          onChange={(e) => updateRule(groupIndex, ruleIndex, { ...rule, values: parseCsvValues(e.target.value) })}
                          placeholder={field === 'tags' ? 'tag_one, tag_two' : '#ff0000, #00ff00'}
                        />
                      ) : isBoolean ? (
                        <select
                          value={String(Boolean(rule.value))}
                          onChange={(e) => updateRule(groupIndex, ruleIndex, { ...rule, value: e.target.value === 'true' })}
                        >
                          <option value="true">True</option>
                          <option value="false">False</option>
                        </select>
                      ) : isEnum && field === 'shape' ? (
                        <select
                          value={typeof rule.value === 'string' ? rule.value : 'landscape'}
                          onChange={(e) => updateRule(groupIndex, ruleIndex, { ...rule, value: e.target.value })}
                        >
                          <option value="landscape">landscape</option>
                          <option value="portrait">portrait</option>
                          <option value="square">square</option>
                        </select>
                      ) : isEnum && field === 'file_type' ? (
                        <select
                          value={typeof rule.value === 'string' ? rule.value : 'image'}
                          onChange={(e) => updateRule(groupIndex, ruleIndex, { ...rule, value: e.target.value })}
                        >
                          <option value="image">image</option>
                          <option value="video">video</option>
                          <option value="audio">audio</option>
                          <option value="image/png">image/png</option>
                          <option value="image/jpeg">image/jpeg</option>
                          <option value="video/mp4">video/mp4</option>
                        </select>
                      ) : (
                        <div style={{ display: 'grid', gridTemplateColumns: isBetween ? '1fr 1fr' : '1fr', gap: 8 }}>
                          <GlassInput
                            type={isNumeric ? 'number' : isDate ? 'date' : 'text'}
                            value={rule.value == null ? '' : String(rule.value)}
                            onChange={(e) => updateRule(groupIndex, ruleIndex, {
                              ...rule,
                              value: isNumeric ? Number(e.target.value || 0) : e.target.value,
                            })}
                            placeholder="Value"
                          />
                          {isBetween && (
                            <GlassInput
                              type={isNumeric ? 'number' : 'text'}
                              value={rule.value2 == null ? '' : String(rule.value2)}
                              onChange={(e) => updateRule(groupIndex, ruleIndex, {
                                ...rule,
                                value2: isNumeric ? Number(e.target.value || 0) : e.target.value,
                              })}
                              placeholder="And"
                            />
                          )}
                        </div>
                      )}
                    </div>
                  );
                })}

                <button className={modalStyles.btn} type="button" onClick={() => addRule(groupIndex)}>Add Rule</button>
              </div>
            ))}
          </div>
        </div>
      </div>
    </GlassModal>
  );
}

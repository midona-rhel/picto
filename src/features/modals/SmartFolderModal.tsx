/**
 * SmartFolderModal — create or edit a smart folder.
 * Uses the canonical SmartFolderPredicate shape end to end.
 * Rebuilt with shared UI components: GlassModal, GlassInput, CmSelect, ToggleSwitch,
 * ColorPicker, IconPicker, and the RuleGroupEditor / RuleEditor sub-components.
 */

import { useState, useEffect, useCallback, useRef } from 'react';
import { createPortal } from 'react-dom';
import { IconPlus, IconFolder } from '@tabler/icons-react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput, GlassTextarea } from '../../shared/ui/GlassInput';
import { ColorPicker } from '../../shared/ui/ColorPicker';
import { IconPicker } from '../../shared/ui/IconPicker';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import type {
  SmartFolderCommandPayload,
  SmartFolderPredicate,
  SmartFolderPredicateGroup,
  SmartFolderPredicateRule,
} from '../../shared/types/canonical';
import { RuleGroupEditor } from './smart-folder/RuleGroupEditor';
import { getFieldDef, defaultOperator, defaultValue, isListField, FIELD_DEFS } from './smart-folder/fieldConfig';

// ── Icon picker popover — compact button that opens a floating dropdown ──

function IconPickerPopover({ value, onChange }: { value: string | null; onChange: (v: string | null) => void }) {
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const [pos, setPos] = useState({ top: 0, left: 0, width: 280 });

  const handleOpen = () => {
    if (btnRef.current) {
      const rect = btnRef.current.getBoundingClientRect();
      setPos({ top: rect.bottom + 4, left: rect.left, width: Math.max(280, rect.width) });
    }
    setOpen(!open);
  };

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        onClick={handleOpen}
        style={{
          display: 'flex', alignItems: 'center', gap: 6,
          height: 32, padding: '0 10px',
          border: '1px solid var(--color-border-primary)',
          borderRadius: 'var(--radius-sm)',
          background: 'var(--color-black-20)',
          color: 'var(--color-text-primary)',
          fontSize: 'var(--font-size-md)',
          cursor: 'pointer', width: '100%',
        }}
      >
        {value ? <DynamicIcon name={value} size={16} /> : <IconFolder size={16} stroke={1.2} />}
        <span style={{ flex: 1, textAlign: 'left' }}>{value ?? 'Default'}</span>
      </button>
      {open && createPortal(
        <>
          <div style={{ position: 'fixed', inset: 0, zIndex: 9998 }} onClick={() => setOpen(false)} />
          <div style={{
            position: 'fixed', top: pos.top, left: pos.left,
            width: pos.width, maxHeight: 300, overflowY: 'auto',
            zIndex: 9999,
            background: 'var(--glass-bg)', backdropFilter: 'var(--glass-blur)',
            border: '1px solid var(--color-border-secondary)',
            borderRadius: 'var(--radius-md)',
            boxShadow: 'var(--shadow-panel)',
            padding: 8,
          }}>
            <IconPicker value={value} onChange={(v) => { onChange(v); setOpen(false); }} />
          </div>
        </>,
        document.body,
      )}
    </>
  );
}

// ── Helpers ──────────────────────────────────────────────────────

function defaultRule(): SmartFolderPredicateRule {
  return { field: 'tags', op: 'include_any', values: [] };
}

function defaultGroup(): SmartFolderPredicateGroup {
  return { match_mode: 'all', negate: false, rules: [defaultRule()] };
}

function emptyPredicate(): SmartFolderPredicate {
  return { groups: [defaultGroup()] };
}

function normalizeRule(rule: SmartFolderPredicateRule): SmartFolderPredicateRule {
  const validKeys = FIELD_DEFS.map((f) => f.key);
  const field = validKeys.includes(rule.field) ? rule.field : 'tags';
  const def = getFieldDef(field);
  const list = isListField(field);

  return {
    field,
    op: rule.op && def.operators.some((o) => o.value === rule.op)
      ? rule.op
      : defaultOperator(field),
    value: list ? undefined : (rule.value ?? defaultValue(field)),
    value2: rule.value2 ?? undefined,
    values: list ? (Array.isArray(rule.values) ? rule.values : []) : undefined,
  };
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

// ── Props ────────────────────────────────────────────────────────

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

// ── Component ────────────────────────────────────────────────────

export function SmartFolderModal({
  open, onClose, onSave, initial, mode = 'create',
}: SmartFolderModalProps) {
  const [name, setName] = useState('');
  const [icon, setIcon] = useState<string | null>(null);
  const [color, setColor] = useState<string | null>(null);
  const [notes, setNotes] = useState('');
  const [predicate, setPredicate] = useState<SmartFolderPredicate>(emptyPredicate());

  // Snapshot of original predicate for revert on cancel
  const originalPredicateRef = useRef<string>('');
  const livePreviewTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!open) return;
    setName(initial?.name ?? '');
    setIcon(initial?.icon ?? null);
    setColor(initial?.color ?? null);
    setNotes(initial?.notes ?? '');
    const pred = normalizePredicate(initial?.predicate);
    setPredicate(pred);
    originalPredicateRef.current = JSON.stringify(initial?.predicate ?? { groups: [] });
  }, [open, initial]);

  // Live preview: debounce-save predicate changes in edit mode so grid updates
  useEffect(() => {
    if (!open || mode !== 'edit' || !initial?.id) return;
    if (livePreviewTimerRef.current) clearTimeout(livePreviewTimerRef.current);
    livePreviewTimerRef.current = setTimeout(() => {
      const payload = buildPayload({
        id: initial.id,
        name: name.trim() || initial.name || 'Smart Folder',
        parent_id: initial.parent_id ?? null,
        icon, color,
        notes: notes.trim() ? notes.trim() : null,
        predicate,
        sort_field: initial.sort_field ?? null,
        sort_order: initial.sort_order ?? null,
        display_order: initial.display_order ?? null,
      });
      void smartFoldersController.update(initial.id!, payload);
    }, 500);
    return () => { if (livePreviewTimerRef.current) clearTimeout(livePreviewTimerRef.current); };
  }, [predicate, open, mode, initial, name, icon, color, notes]);

  // Revert predicate on close without save (cancel)
  const handleClose = useCallback(() => {
    if (mode === 'edit' && initial?.id && originalPredicateRef.current) {
      // Revert to original
      const origPred = normalizePredicate(JSON.parse(originalPredicateRef.current));
      const revertPayload = buildPayload({
        id: initial.id,
        name: initial.name || '',
        parent_id: initial.parent_id ?? null,
        icon: initial.icon ?? null,
        color: initial.color ?? null,
        notes: initial.notes ?? null,
        predicate: origPred,
        sort_field: initial.sort_field ?? null,
        sort_order: initial.sort_order ?? null,
        display_order: initial.display_order ?? null,
      });
      void smartFoldersController.update(initial.id, revertPayload);
    }
    onClose();
  }, [mode, initial, onClose]);

  const handleSave = useCallback(() => {
    if (!name.trim()) return;
    // Update the snapshot so handleClose won't revert
    originalPredicateRef.current = JSON.stringify(predicate);
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

  const handleGroupChange = useCallback((index: number, group: SmartFolderPredicateGroup) => {
    setPredicate((current) => ({
      groups: current.groups.map((g, i) => (i === index ? group : g)),
    }));
  }, []);

  const handleGroupRemove = useCallback((index: number) => {
    setPredicate((current) => ({
      groups: current.groups.filter((_, i) => i !== index),
    }));
  }, []);

  const handleGroupAdd = useCallback(() => {
    setPredicate((current) => ({ groups: [...current.groups, defaultGroup()] }));
  }, []);

  return (
    <GlassModal
      open={open}
      onClose={handleClose}
      title={mode === 'create' ? 'New Smart Folder' : 'Edit Smart Folder'}
      size="lg"
      footer={(
        <>
          <button className={modalStyles.btn} onClick={handleClose} type="button">Cancel</button>
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
      <div className={modalStyles.stack}>
        {/* Name */}
        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Name</label>
          <GlassInput
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Smart Folder Name"
            autoFocus
          />
        </div>

        {/* Icon + Color */}
        <div className={modalStyles.grid2}>
          <div className={modalStyles.field}>
            <label className={modalStyles.fieldLabel}>Icon</label>
            <IconPickerPopover value={icon} onChange={setIcon} />
          </div>
          <div className={modalStyles.field}>
            <label className={modalStyles.fieldLabel}>Color</label>
            <ColorPicker value={color} onChange={setColor} />
          </div>
        </div>

        {/* Notes */}
        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Notes</label>
          <GlassTextarea
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            placeholder="Notes..."
            rows={3}
          />
        </div>

        <div className={modalStyles.separator} />

        {/* Rules */}
        <div className={modalStyles.section}>
          <div className={modalStyles.rowSpread}>
            <span className={modalStyles.sectionLabel}>Rules</span>
            <button
              className={modalStyles.btn}
              onClick={handleGroupAdd}
              type="button"
              style={{ gap: 4 }}
            >
              <IconPlus size={14} />
              Add Group
            </button>
          </div>
          <div className={modalStyles.stackSm}>
            {predicate.groups.map((group, index) => (
              <RuleGroupEditor
                key={index}
                group={group}
                onChange={(next) => handleGroupChange(index, next)}
                onRemove={() => handleGroupRemove(index)}
                canRemove={predicate.groups.length > 1}
              />
            ))}
          </div>
        </div>
      </div>
    </GlassModal>
  );
}

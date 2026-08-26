/**
 * SmartFolderModal — create or edit a smart folder.
 * Uses the canonical SmartFolderPredicate shape end to end.
 * Rebuilt with shared UI components: GlassModal, GlassInput, CmSelect, ToggleSwitch,
 * ColorPicker, IconPicker, and the RuleGroupEditor / RuleEditor sub-components.
 */

import { useState, useEffect, useCallback, useRef } from 'react';
import { createPortal } from 'react-dom';
import { IconChevronDown, IconFolder } from '@tabler/icons-react';
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
import styles from './SmartFolderModal.module.css';

// ── Icon picker popover — compact button that opens a floating dropdown ──

function IconPickerPopover({ value, onChange }: { value: string | null; onChange: (v: string | null) => void }) {
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const [pos, setPos] = useState({ top: 0, left: 0 });

  const handleOpen = () => {
    if (btnRef.current) {
      const rect = btnRef.current.getBoundingClientRect();
      setPos({
        top: Math.max(4, Math.min(rect.bottom + 4, window.innerHeight - 312)),
        left: Math.max(4, Math.min(rect.left, window.innerWidth - 264)),
      });
    }
    setOpen(!open);
  };

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        aria-label="Change icon"
        onClick={handleOpen}
        className={styles.iconTrigger}
      >
        {value ? <DynamicIcon name={value} size={16} /> : <IconFolder size={16} stroke={1.2} />}
        <span className={styles.iconTriggerLabel}>{value ?? 'Default'}</span>
        <IconChevronDown className={styles.iconTriggerChevron} size={14} />
      </button>
      {open && createPortal(
        <>
          <div className={styles.pickerBackdrop} onPointerDown={() => setOpen(false)} />
          <div className={styles.pickerPopover} style={{ top: pos.top, left: pos.left }}>
            <IconPicker compact value={value} onChange={(v) => { onChange(v); setOpen(false); }} />
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
    display_order?: number | null;
  };
  mode?: 'create' | 'edit';
  editor?: 'all' | 'details' | 'rules';
}

// ── Component ────────────────────────────────────────────────────

export function SmartFolderModal({
  open, onClose, onSave, initial, mode = 'create', editor = mode === 'create' ? 'all' : 'details',
}: SmartFolderModalProps) {
  const [name, setName] = useState('');
  const [icon, setIcon] = useState<string | null>(null);
  const [color, setColor] = useState<string | null>(null);
  const [notes, setNotes] = useState('');
  const [predicate, setPredicate] = useState<SmartFolderPredicate>(emptyPredicate());

  // Snapshot of original predicate for revert on cancel
  const originalPredicateRef = useRef<string>('');
  const livePreviewTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const showDetails = editor !== 'rules';
  const showRules = editor !== 'details';
  const title = mode === 'create'
    ? 'New Smart Folder'
    : editor === 'rules' ? 'Edit Rules' : 'Edit Smart Folder';

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
    if (!open || mode !== 'edit' || !initial?.id || !showRules) return;
    if (livePreviewTimerRef.current) clearTimeout(livePreviewTimerRef.current);
    livePreviewTimerRef.current = setTimeout(() => {
      const payload = buildPayload({
        id: initial.id,
        name: name.trim() || initial.name || 'Smart Folder',
        parent_id: initial.parent_id ?? null,
        icon, color,
        notes: notes.trim() ? notes.trim() : null,
        predicate,
        display_order: initial.display_order ?? null,
      });
      void smartFoldersController.update(initial.id!, payload);
    }, 500);
    return () => { if (livePreviewTimerRef.current) clearTimeout(livePreviewTimerRef.current); };
  }, [predicate, open, mode, initial, name, icon, color, notes, showRules]);

  // Revert predicate on close without save (cancel)
  const handleClose = useCallback(() => {
    if (mode === 'edit' && showRules && initial?.id && originalPredicateRef.current) {
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
        display_order: initial.display_order ?? null,
      });
      void smartFoldersController.update(initial.id, revertPayload);
    }
    onClose();
  }, [mode, showRules, initial, onClose]);

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
      title={title}
      size={showRules ? 'lg' : 'md'}
      panelClassName={`${styles.modal} ${showRules && !showDetails ? styles.rulesModal : ''}`}
      footer={(
        <>
          <button
            data-modal-primary="true"
            className={`${modalStyles.btn} ${modalStyles.btnPrimary}`}
            onClick={handleSave}
            disabled={!name.trim()}
            type="button"
          >
            {mode === 'create' ? 'Create' : 'Save Changes'}
          </button>
          <button className={modalStyles.btn} onClick={handleClose} type="button">Cancel</button>
        </>
      )}
    >
      <div className={styles.form}>
        {showDetails && (
          <>
            <div className={styles.section}>
              <label className={styles.label}>Name</label>
              <GlassInput
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Smart Folder Name"
                autoFocus
              />
            </div>

            <div className={styles.section}>
              <div className={styles.appearanceGrid}>
                <div className={styles.compactField}>
                  <label className={styles.label}>Icon</label>
                  <IconPickerPopover value={icon} onChange={setIcon} />
                </div>
                <div className={styles.compactField}>
                  <label className={styles.label}>Color</label>
                  <ColorPicker value={color} onChange={setColor} />
                </div>
              </div>
            </div>

            <div className={styles.section}>
              <label className={styles.label}>Notes</label>
              <GlassTextarea
                value={notes}
                onChange={(e) => setNotes(e.target.value)}
                placeholder="Notes..."
                rows={3}
              />
            </div>
          </>
        )}

        {showRules && (
          <div className={styles.section}>
            <div className={styles.conditions}>
              {predicate.groups.map((group, index) => (
                <RuleGroupEditor
                  key={index}
                  group={group}
                  onChange={(next) => handleGroupChange(index, next)}
                  onRemove={() => handleGroupRemove(index)}
                  onAdd={handleGroupAdd}
                  canRemove={predicate.groups.length > 1}
                />
              ))}
            </div>
          </div>
        )}
      </div>
    </GlassModal>
  );
}

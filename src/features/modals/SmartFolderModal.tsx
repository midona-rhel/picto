/**
 * SmartFolderModal — create or edit a smart folder.
 * TODO: Rule group editor for predicates.
 */

import { useState, useEffect, useCallback } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';
import { ColorPicker } from '../../shared/ui/ColorPicker';
import { IconPicker } from '../../shared/ui/IconPicker';

export interface SmartFolderModalProps {
  open: boolean;
  onClose: () => void;
  onSave: (data: { name: string; icon: string | null; color: string | null }) => void;
  initial?: { name?: string; icon?: string | null; color?: string | null; id?: number };
  mode?: 'create' | 'edit';
}

export function SmartFolderModal({
  open, onClose, onSave, initial, mode = 'create',
}: SmartFolderModalProps) {
  const [name, setName] = useState('');
  const [icon, setIcon] = useState<string | null>(null);
  const [color, setColor] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setName(initial?.name ?? '');
      setIcon(initial?.icon ?? null);
      setColor(initial?.color ?? null);
    }
  }, [open, initial]);

  const handleSave = useCallback(() => {
    if (!name.trim()) return;
    onSave({ name: name.trim(), icon, color });
  }, [name, icon, color, onSave]);

  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title={mode === 'create' ? 'New Smart Folder' : 'Edit Smart Folder'}
      size="md"
      footer={
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
      }
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
          <label className={modalStyles.fieldLabel}>Icon</label>
          <IconPicker value={icon} onChange={setIcon} />
        </div>

        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Color</label>
          <ColorPicker value={color} onChange={setColor} />
        </div>

        <div className={modalStyles.separator} />

        <p className={modalStyles.helpText}>
          TODO: Rule group editor for smart folder predicates.
        </p>
      </div>
    </GlassModal>
  );
}

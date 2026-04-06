/**
 * FolderWatchModal — configure auto-import from a watched disk folder.
 */

import { useState, useEffect, useCallback } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';
import { CmSelect } from '../../shared/ui/CmSelect/CmSelect';

const STATUS_OPTIONS = [
  { value: 'inherit', label: 'Inherit' },
  { value: 'inbox', label: 'Inbox' },
  { value: 'active', label: 'Active' },
];

export interface FolderWatchConfig {
  watchPath: string;
  enabled: boolean;
  subfolders: boolean;
  importStatusMode: string;
}

export interface FolderWatchModalProps {
  open: boolean;
  onClose: () => void;
  onSave: (config: FolderWatchConfig) => void;
  onRemove?: () => void;
  initial?: Partial<FolderWatchConfig>;
}

export function FolderWatchModal({
  open, onClose, onSave, onRemove, initial,
}: FolderWatchModalProps) {
  const [watchPath, setWatchPath] = useState('');
  const [enabled, setEnabled] = useState(true);
  const [subfolders, setSubfolders] = useState(false);
  const [importStatusMode, setImportStatusMode] = useState('inherit');

  useEffect(() => {
    if (open) {
      setWatchPath(initial?.watchPath ?? '');
      setEnabled(initial?.enabled ?? true);
      setSubfolders(initial?.subfolders ?? false);
      setImportStatusMode(initial?.importStatusMode ?? 'inherit');
    }
  }, [open, initial]);

  const browse = useCallback(async () => {
    try {
      const result = await (window as any).picto.dialog.open({
        properties: ['openDirectory'],
        multiple: false,
        title: 'Select folder to watch',
      });
      if (result) setWatchPath(typeof result === 'string' ? result : result[0]);
    } catch {}
  }, []);

  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title="Auto-Import Folder"
      size="md"
      footer={
        <>
          {onRemove && (
            <button className={`${modalStyles.btn} ${modalStyles.btnDanger}`} onClick={onRemove} type="button" style={{ marginRight: 'auto' }}>
              Remove Watch
            </button>
          )}
          <button className={modalStyles.btn} onClick={onClose} type="button">Cancel</button>
          <button
            className={`${modalStyles.btn} ${modalStyles.btnPrimary}`}
            onClick={() => onSave({ watchPath, enabled, subfolders, importStatusMode })}
            disabled={!watchPath}
            type="button"
          >
            Save
          </button>
        </>
      }
    >
      <div className={modalStyles.stack}>
        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Watch Folder</label>
          <div className={modalStyles.fieldRow}>
            <GlassInput value={watchPath} readOnly placeholder="Select a folder..." style={{ flex: 1 }} />
            <button className={modalStyles.btn} onClick={browse} type="button">Browse</button>
          </div>
        </div>

        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>Enable Watch</span>
          <ToggleSwitch on={enabled} onChange={() => setEnabled(!enabled)} />
        </div>

        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>Watch Subfolders</span>
          <ToggleSwitch on={subfolders} onChange={() => setSubfolders(!subfolders)} />
        </div>

        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Import Status</label>
          <CmSelect value={importStatusMode} options={STATUS_OPTIONS} onChange={setImportStatusMode} width={160} />
        </div>

        <p className={modalStyles.helpText}>
          New files added to this folder will be automatically imported into the library.
        </p>
      </div>
    </GlassModal>
  );
}

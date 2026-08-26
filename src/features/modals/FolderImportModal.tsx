import { useEffect, useState } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';

export interface FolderImportOptions {
  preserveStructure: boolean;
  includeSubfolders: boolean;
  expandArchives: boolean;
  includeFoldersWithoutMedia: boolean;
}

export function FolderImportModal({
  open,
  path,
  onClose,
  onImport,
}: {
  open: boolean;
  path: string;
  onClose: () => void;
  onImport: (options: FolderImportOptions) => void;
}) {
  const [options, setOptions] = useState<FolderImportOptions>({
    preserveStructure: true,
    includeSubfolders: true,
    expandArchives: true,
    includeFoldersWithoutMedia: false,
  });

  useEffect(() => {
    if (!open) return;
    setOptions({
      preserveStructure: true,
      includeSubfolders: true,
      expandArchives: true,
      includeFoldersWithoutMedia: false,
    });
  }, [open, path]);

  const toggle = (key: keyof FolderImportOptions) => {
    setOptions((current) => ({ ...current, [key]: !current[key] }));
  };

  const folderName = path.split(/[\\/]/).filter(Boolean).pop() ?? 'folder';
  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title="Import Folder"
      size="sm"
      footer={(
        <>
          <button className={modalStyles.btn} onClick={onClose} type="button">Cancel</button>
          <button
            className={`${modalStyles.btn} ${modalStyles.btnPrimary}`}
            onClick={() => onImport(options)}
            type="button"
            data-modal-primary="true"
          >
            Import
          </button>
        </>
      )}
    >
      <div className={modalStyles.stack}>
        <p className={modalStyles.helpText}>Import <strong>{folderName}</strong></p>
        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>Preserve folder structure</span>
          <ToggleSwitch on={options.preserveStructure} onChange={() => toggle('preserveStructure')} />
        </div>
        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>Include subfolders</span>
          <ToggleSwitch on={options.includeSubfolders} onChange={() => toggle('includeSubfolders')} />
        </div>
        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>Extract ZIP archives</span>
          <ToggleSwitch on={options.expandArchives} onChange={() => toggle('expandArchives')} />
        </div>
        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>Include folders without media</span>
          <ToggleSwitch
            on={options.includeFoldersWithoutMedia}
            onChange={() => toggle('includeFoldersWithoutMedia')}
            disabled={!options.preserveStructure}
          />
        </div>
        <p className={modalStyles.helpText}>
          Folders whose subtree contains no supported media are skipped by default.
        </p>
      </div>
    </GlassModal>
  );
}

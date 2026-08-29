import { useEffect, useState } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';
import { analyzeFolderTree } from '../../platform/folderApi';
import type { FolderTreeAnalysis } from '../../shared/types/generated/application/FolderTreeAnalysis';
import { folderConsolidationMessage } from '../folders/folderDepthAnalysis';

export interface FolderImportOptions {
  preserveStructure: boolean;
  includeSubfolders: boolean;
  includeFoldersWithoutMedia: boolean;
  watchSourceFolder: boolean;
}

export function FolderImportModal({
  open,
  path,
  onClose,
  onImport,
  targetFolderId,
}: {
  open: boolean;
  path: string;
  onClose: () => void;
  onImport: (options: FolderImportOptions) => void;
  targetFolderId: number | null;
}) {
  const [options, setOptions] = useState<FolderImportOptions>({
    preserveStructure: true,
    includeSubfolders: true,
    includeFoldersWithoutMedia: false,
    watchSourceFolder: false,
  });
  const [analysis, setAnalysis] = useState<FolderTreeAnalysis | null>(null);
  const [analyzing, setAnalyzing] = useState(false);
  const [analysisError, setAnalysisError] = useState<string | null>(null);
  const consolidationMessage = folderConsolidationMessage(analysis);

  useEffect(() => {
    if (!open) return;
    setOptions({
      preserveStructure: true,
      includeSubfolders: true,
      includeFoldersWithoutMedia: false,
      watchSourceFolder: false,
    });
  }, [open, path]);

  useEffect(() => {
    if (!open || !path || !options.preserveStructure) {
      setAnalysis(null);
      setAnalysisError(null);
      return;
    }
    let cancelled = false;
    setAnalyzing(true);
    setAnalysisError(null);
    void analyzeFolderTree({
      path,
      destination_folder_id: targetFolderId,
      include_subfolders: options.includeSubfolders,
      include_source_root: true,
    }).then((result) => {
      if (!cancelled) setAnalysis(result);
    }).catch((reason) => {
      if (!cancelled) {
        setAnalysis(null);
        setAnalysisError(reason instanceof Error ? reason.message : String(reason));
      }
    }).finally(() => {
      if (!cancelled) setAnalyzing(false);
    });
    return () => { cancelled = true; };
  }, [open, options.includeSubfolders, options.preserveStructure, path, targetFolderId]);

  const toggle = (key: keyof FolderImportOptions) => {
    setOptions((current) => {
      const enabled = !current[key];
      if (key === 'watchSourceFolder' && enabled) {
        return { ...current, preserveStructure: true, watchSourceFolder: true };
      }
      return { ...current, [key]: enabled };
    });
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
            disabled={analyzing || analysisError != null}
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
        {analyzing && <p className={modalStyles.helpText}>Checking the folder structure...</p>}
        {consolidationMessage && (
          <div className={modalStyles.warningBox} role="status">
            {consolidationMessage}
          </div>
        )}
        {analysisError && (
          <div className={modalStyles.warningBox} role="alert">
            Picto could not inspect this folder. Choose it again or check that it is still available.
          </div>
        )}
        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>Preserve folder structure</span>
          <ToggleSwitch on={options.preserveStructure} onChange={() => toggle('preserveStructure')} />
        </div>
        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>Include subfolders</span>
          <ToggleSwitch on={options.includeSubfolders} onChange={() => toggle('includeSubfolders')} />
        </div>
        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>Include folders without media</span>
          <ToggleSwitch
            on={options.includeFoldersWithoutMedia}
            onChange={() => toggle('includeFoldersWithoutMedia')}
            disabled={!options.preserveStructure}
          />
        </div>
        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>Watch this folder</span>
          <ToggleSwitch
            on={options.watchSourceFolder}
            onChange={() => toggle('watchSourceFolder')}
          />
        </div>
        <p className={modalStyles.helpText}>
          Automatically import new media added to this folder. Watching uses the same subfolder setting above.
        </p>
        <p className={modalStyles.helpText}>
          Folders whose subtree contains no supported media are skipped by default.
        </p>
      </div>
    </GlassModal>
  );
}

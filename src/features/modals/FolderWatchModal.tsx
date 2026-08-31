/**
 * FolderWatchModal — configure auto-import from a watched disk folder.
 */

import { useState, useEffect, useCallback } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';
import { CmSelect } from '../../shared/ui/CmSelect/CmSelect';
import { analyzeFolderTree } from '../../platform/folderApi';
import type { FolderTreeAnalysis } from '../../shared/types/generated/application/FolderTreeAnalysis';
import { folderConsolidationMessage } from '../folders/folderDepthAnalysis';
import { t } from '../../i18n';

const STATUS_OPTIONS = [
  { value: 'inherit', label: t("Inherit") },
  { value: 'inbox', label: t("Inbox") },
  { value: 'active', label: t("Active") },
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
  folderId: number | null;
}

export function FolderWatchModal({
  open, onClose, onSave, onRemove, initial, folderId,
}: FolderWatchModalProps) {
  const [watchPath, setWatchPath] = useState('');
  const [enabled, setEnabled] = useState(true);
  const [subfolders, setSubfolders] = useState(false);
  const [importStatusMode, setImportStatusMode] = useState('inherit');
  const [analysis, setAnalysis] = useState<FolderTreeAnalysis | null>(null);
  const [analyzing, setAnalyzing] = useState(false);
  const [analysisError, setAnalysisError] = useState<string | null>(null);
  const consolidationMessage = folderConsolidationMessage(analysis);

  useEffect(() => {
    if (open) {
      setWatchPath(initial?.watchPath ?? '');
      setEnabled(initial?.enabled ?? true);
      setSubfolders(initial?.subfolders ?? false);
      setImportStatusMode(initial?.importStatusMode ?? 'inherit');
      setAnalysis(null);
      setAnalysisError(null);
    }
  }, [open, initial]);

  useEffect(() => {
    if (!open || !watchPath || folderId == null) return;
    let cancelled = false;
    setAnalyzing(true);
    setAnalysisError(null);
    void analyzeFolderTree({
      path: watchPath,
      destination_folder_id: folderId,
      include_subfolders: subfolders,
      include_source_root: false,
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
  }, [folderId, open, subfolders, watchPath]);

  const browse = useCallback(async () => {
    try {
      const result = await (window as any).picto.dialog.open({
        properties: ['openDirectory'],
        multiple: false,
        title: t("Select folder to watch"),
      });
      if (result) setWatchPath(typeof result === 'string' ? result : result[0]);
    } catch {}
  }, []);

  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title={t("Auto-Import Folder")}
      size="md"
      footer={
        <>
          {onRemove && (
            <button className={`${modalStyles.btn} ${modalStyles.btnDanger}`} onClick={onRemove} type="button" style={{ marginRight: 'auto' }}>
              {t("Remove Watch")}</button>
          )}
          <button className={modalStyles.btn} onClick={onClose} type="button">{t("Cancel")}</button>
          <button
            data-modal-primary="true"
            className={`${modalStyles.btn} ${modalStyles.btnPrimary}`}
            onClick={() => onSave({ watchPath, enabled, subfolders, importStatusMode })}
            disabled={!watchPath || analyzing || analysisError != null}
            type="button"
          >
            {t("Save")}</button>
        </>
      }
    >
      <div className={modalStyles.stack}>
        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>{t("Watch Folder")}</label>
          <div className={modalStyles.fieldRow}>
            <GlassInput value={watchPath} readOnly placeholder={t("Select a folder...")} style={{ flex: 1 }} />
            <button className={modalStyles.btn} onClick={browse} type="button">{t("Browse")}</button>
          </div>
        </div>

        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>{t("Enable Watch")}</span>
          <ToggleSwitch on={enabled} onChange={() => setEnabled(!enabled)} />
        </div>

        <div className={modalStyles.rowSpread}>
          <span className={modalStyles.fieldLabel}>{t("Watch Subfolders")}</span>
          <ToggleSwitch on={subfolders} onChange={() => setSubfolders(!subfolders)} />
        </div>

        {analyzing && <p className={modalStyles.helpText}>{t("Checking the folder structure...")}</p>}
        {consolidationMessage && (
          <div className={modalStyles.warningBox} role="status">
            {consolidationMessage}
          </div>
        )}
        {analysisError && (
          <div className={modalStyles.warningBox} role="alert">
            {t("Picto could not inspect this folder. Choose it again or check that it is still available.")}</div>
        )}

        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>{t("Import Status")}</label>
          <CmSelect value={importStatusMode} options={STATUS_OPTIONS} onChange={setImportStatusMode} width={160} />
        </div>

        <p className={modalStyles.helpText}>
          {t("New files added to this folder will be automatically imported into the library.")}</p>
      </div>
    </GlassModal>
  );
}

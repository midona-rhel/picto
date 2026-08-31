import { useEffect, useState } from 'react';
import { IconPackageExport, IconPackageImport } from '@tabler/icons-react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import type { PictoPackModalState } from '../../state/modals';
import { exportPictoPack, importPictoPack } from '../../platform/pictoPackApi';
import { showErrorNotification, showSuccessNotification } from '../../shared/lib/notifications';
import { t } from '../../i18n';

interface Props {
  state: PictoPackModalState;
  onClose: () => void;
}

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let size = value / 1024;
  let unit = units[0];
  for (let index = 1; size >= 1024 && index < units.length; index += 1) {
    size /= 1024;
    unit = units[index];
  }
  return `${size.toFixed(size >= 10 ? 1 : 2)} ${unit}`;
}

function safePackName(value: string): string {
  const cleaned = value.trim().replace(/[\\/:*?"<>|]+/g, '-').replace(/\.+$/g, '');
  return cleaned || 'Picto Pack';
}

export function PictoPackModal({ state, onClose }: Props) {
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (!state.open) setBusy(false); }, [state.open]);
  if (!state.open) return null;

  const importing = state.mode === 'import';
  const summary = importing ? state.summary : null;
  const run = async () => {
    setBusy(true);
    try {
      if (state.mode === 'import') {
        const result = await importPictoPack(state.path);
        showSuccessNotification({
          title: t("Picto Pack imported"),
          message: t("Imported {value0} items and {value1} media files.", { value0: result.imported_roots, value1: result.imported_media }),
        });
      } else {
        const outputPath = await (window as any).picto.dialog.save({
          title: t("Export Picto Pack"),
          defaultPath: `${safePackName(state.suggestedName)}.picto-pack`,
          filters: [{ name: t("Picto Pack"), extensions: ['picto-pack'] }],
        });
        if (!outputPath) { setBusy(false); return; }
        const result = await exportPictoPack(state.source, outputPath);
        showSuccessNotification({
          title: t("Picto Pack exported"),
          message: t("Exported {value0} items and {value1} media files.", { value0: result.summary.root_count, value1: result.summary.media_count }),
        });
      }
      onClose();
    } catch (reason) {
      showErrorNotification({
        title: importing ? t("Could not import Picto Pack") : t("Could not export Picto Pack"),
        message: reason instanceof Error ? reason.message : String(reason),
      });
      setBusy(false);
    }
  };

  return (
    <GlassModal
      open
      onClose={busy ? () => {} : onClose}
      title={importing ? t("Import Picto Pack") : t("Export Picto Pack")}
      size="md"
      footer={(
        <>
          <button className={modalStyles.btn} onClick={onClose} disabled={busy} type="button">{t("Cancel")}</button>
          <button
            data-modal-primary="true"
            className={`${modalStyles.btn} ${modalStyles.btnPrimary}`}
            onClick={() => { void run(); }}
            disabled={busy}
            type="button"
          >
            {busy ? t("Working...") : importing ? t("Import") : t("Choose Destination...")}
          </button>
        </>
      )}
    >
      <div className={modalStyles.stack}>
        <div style={{ display: 'flex', gap: 14, alignItems: 'flex-start' }}>
          <div style={{ padding: 10, borderRadius: 12, background: 'var(--surface-raised, rgba(255,255,255,.06))' }}>
            {importing ? <IconPackageImport size={28} /> : <IconPackageExport size={28} />}
          </div>
          <div style={{ display: 'grid', gap: 7 }}>
            <strong>{importing ? summary?.name : t("Portable library package")}</strong>
            <span className={modalStyles.fieldLabel}>
              {t("Picto Packs keep original media, collections, names, notes, dates, ratings, tags, source links, and included folder definitions.")}
            </span>
            <span className={modalStyles.fieldLabel}>
              {t("Subscriptions, provider history, authentication, and folder watch paths are never included.")}
            </span>
            {state.mode === 'export' && state.source.kind === 'smart_folder' && (
              <span className={modalStyles.fieldLabel}>
                {t("Smart-folder exports contain only the current matching items. The smart-folder rule and container are not included.")}
              </span>
            )}
          </div>
        </div>
        <div className={modalStyles.separator} />
        {summary ? (
          <div className={modalStyles.stack}>
            <div className={modalStyles.rowSpread}><span>{t("Items")}</span><strong>{summary.root_count}</strong></div>
            <div className={modalStyles.rowSpread}><span>{t("Media files")}</span><strong>{summary.media_count}</strong></div>
            <div className={modalStyles.rowSpread}><span>{t("Folders")}</span><strong>{summary.folder_count}</strong></div>
            <div className={modalStyles.rowSpread}><span>{t("Smart folders")}</span><strong>{summary.smart_folder_count}</strong></div>
            <div className={modalStyles.rowSpread}><span>{t("Original size")}</span><strong>{formatBytes(summary.total_bytes)}</strong></div>
          </div>
        ) : (
          <div className={modalStyles.rowSpread}>
            <span>{t("Selected items")}</span>
            <strong>{state.mode === 'export' ? state.itemCount : 0}</strong>
          </div>
        )}
      </div>
    </GlassModal>
  );
}

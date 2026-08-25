/**
 * ExportModal — configure and export files with format/quality/dimension options.
 */

import { useState, useEffect, useCallback } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';
import { CmSelect } from '../../shared/ui/CmSelect/CmSelect';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';
import type { ExportFormat } from '../../shared/types/generated/application/ExportFormat';

const FORMAT_OPTIONS: Array<{ value: ExportFormat; label: string }> = [
  { value: 'original', label: 'Original Format' },
  { value: 'jpeg', label: 'JPEG' },
  { value: 'png', label: 'PNG' },
  { value: 'webp', label: 'WebP' },
  { value: 'avif', label: 'AVIF' },
];

export interface ExportConfig {
  outputDir: string;
  format: ExportFormat;
  quality: number;
  width: number | null;
  height: number | null;
  keepAspectRatio: boolean;
}

export interface ExportModalProps {
  open: boolean;
  onClose: () => void;
  onExport: (config: ExportConfig) => void;
  fileCount: number;
}

export function ExportModal({ open, onClose, onExport, fileCount }: ExportModalProps) {
  const [outputDir, setOutputDir] = useState('');
  const [format, setFormat] = useState<ExportFormat>('original');
  const [quality, setQuality] = useState(90);
  const [width, setWidth] = useState('');
  const [height, setHeight] = useState('');
  const [keepAspectRatio, setKeepAspectRatio] = useState(true);

  useEffect(() => {
    if (open) {
      setOutputDir(''); setFormat('original'); setQuality(90);
      setWidth(''); setHeight(''); setKeepAspectRatio(true);
    }
  }, [open]);

  const browse = useCallback(async () => {
    try {
      const result = await (window as any).picto.dialog.open({
        properties: ['openDirectory'], multiple: false, title: 'Choose export folder',
      });
      if (result) setOutputDir(typeof result === 'string' ? result : result[0]);
    } catch {}
  }, []);

  const isOriginal = format === 'original';

  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title={`Export ${fileCount} File${fileCount !== 1 ? 's' : ''}`}
      size="md"
      footer={
        <>
          <button className={modalStyles.btn} onClick={onClose} type="button">Cancel</button>
          <button
            data-modal-primary="true"
            className={`${modalStyles.btn} ${modalStyles.btnPrimary}`}
            onClick={() => onExport({
              outputDir, format, quality,
              width: width ? parseInt(width, 10) : null,
              height: height ? parseInt(height, 10) : null,
              keepAspectRatio,
            })}
            disabled={!outputDir}
            type="button"
          >
            Export
          </button>
        </>
      }
    >
      <div className={modalStyles.stack}>
        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Destination</label>
          <div className={modalStyles.fieldRow}>
            <GlassInput value={outputDir} readOnly placeholder="Choose export folder..." style={{ flex: 1 }} />
            <button className={modalStyles.btn} onClick={browse} type="button">Browse</button>
          </div>
        </div>

        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Format</label>
          <CmSelect
            value={format}
            options={FORMAT_OPTIONS}
            onChange={(value) => setFormat(value as ExportFormat)}
            width={180}
          />
        </div>

        {!isOriginal && format !== 'png' && (
          <div className={modalStyles.field}>
            <label className={modalStyles.fieldLabel}>Quality — {quality}%</label>
            <input
              type="range" min={1} max={100} value={quality}
              onChange={(e) => setQuality(parseInt(e.target.value, 10))}
              className={modalStyles.rangeInput}
            />
          </div>
        )}

        {!isOriginal && (
          <>
            <div className={modalStyles.separator} />
            <div className={modalStyles.field}>
              <label className={modalStyles.fieldLabel}>Resize (optional)</label>
              <div className={modalStyles.fieldRow}>
                <GlassInput
                  value={width}
                  onChange={(e) => setWidth(e.target.value.replace(/\D/g, ''))}
                  placeholder="Width"
                  style={{ width: 100 }}
                />
                <span className={modalStyles.inlineLabel}>×</span>
                <GlassInput
                  value={height}
                  onChange={(e) => setHeight(e.target.value.replace(/\D/g, ''))}
                  placeholder="Height"
                  style={{ width: 100 }}
                />
              </div>
            </div>

            <div className={modalStyles.rowSpread}>
              <span className={modalStyles.fieldLabel}>Keep Aspect Ratio</span>
              <ToggleSwitch on={keepAspectRatio} onChange={() => setKeepAspectRatio(!keepAspectRatio)} />
            </div>
          </>
        )}
      </div>
    </GlassModal>
  );
}

/**
 * ExportModal — configure and export files with format/quality/dimension options.
 */

import { useState, useEffect, useCallback } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';
import { CmSelect } from '../../shared/ui/CmSelect/CmSelect';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';

const FORMAT_OPTIONS = [
  { value: 'original', label: 'Original Format' },
  { value: 'jpeg', label: 'JPEG' },
  { value: 'png', label: 'PNG' },
  { value: 'webp', label: 'WebP' },
  { value: 'avif', label: 'AVIF' },
];

export interface ExportConfig {
  outputDir: string;
  format: string;
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
  const [format, setFormat] = useState('original');
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
      <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Destination</label>
          <div className={modalStyles.fieldRow}>
            <GlassInput value={outputDir} readOnly placeholder="Choose export folder..." style={{ flex: 1 }} />
            <button className={modalStyles.btn} onClick={browse} type="button">Browse</button>
          </div>
        </div>

        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Format</label>
          <CmSelect value={format} options={FORMAT_OPTIONS} onChange={setFormat} width={180} />
        </div>

        {!isOriginal && format !== 'png' && (
          <div className={modalStyles.field}>
            <label className={modalStyles.fieldLabel}>Quality — {quality}%</label>
            <input
              type="range" min={1} max={100} value={quality}
              onChange={(e) => setQuality(parseInt(e.target.value, 10))}
              style={{ width: '100%', accentColor: 'var(--color-primary)' }}
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
                <span style={{ color: 'var(--color-text-tertiary)', fontSize: 13 }}>×</span>
                <GlassInput
                  value={height}
                  onChange={(e) => setHeight(e.target.value.replace(/\D/g, ''))}
                  placeholder="Height"
                  style={{ width: 100 }}
                />
              </div>
            </div>

            <div className={modalStyles.fieldRow} style={{ justifyContent: 'space-between' }}>
              <span className={modalStyles.fieldLabel}>Keep Aspect Ratio</span>
              <ToggleSwitch on={keepAspectRatio} onChange={() => setKeepAspectRatio(!keepAspectRatio)} />
            </div>
          </>
        )}
      </div>
    </GlassModal>
  );
}

import { useEffect, useMemo, useState } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';
import styles from './BatchRenameModal.module.css';

type RenameItem = { root_id: number; name: string };
type Mode = 'format' | 'replace';

export function buildBatchRenamePreview(
  items: RenameItem[], mode: Mode, pattern: string, replacement: string, startAt: number,
): RenameItem[] {
  return items.map((item, index) => {
    const sequence = startAt + index;
    const name = mode === 'replace' && pattern.length === 0
      ? item.name
      : mode === 'replace'
      ? item.name.split(pattern).join(replacement)
      : pattern.split('*').join(item.name)
        .replace(/%N{1,4}/g, (token: string) => String(sequence).padStart(token.length - 1, '0'));
    return { root_id: item.root_id, name: name.trim() };
  });
}

export function BatchRenameModal({ open, items, onClose, onRename }: {
  open: boolean;
  items: RenameItem[];
  onClose: () => void;
  onRename: (items: RenameItem[]) => void;
}) {
  const [mode, setMode] = useState<Mode>('format');
  const [pattern, setPattern] = useState('New Name - %N');
  const [replacement, setReplacement] = useState('');
  const [startAt, setStartAt] = useState(1);
  useEffect(() => {
    if (!open) return;
    setMode('format'); setPattern('New Name - %N'); setReplacement(''); setStartAt(1);
  }, [open]);
  const preview = useMemo(
    () => buildBatchRenamePreview(items, mode, pattern, replacement, startAt),
    [items, mode, pattern, replacement, startAt],
  );
  const invalid = (mode === 'replace' && pattern.length === 0)
    || preview.some((item) => !item.name || item.name.length > 255);

  return <GlassModal open={open} onClose={onClose} title={`Batch Rename ${items.length} Items`} size="md" footer={<>
    <button className={modalStyles.btn} onClick={onClose} type="button">Cancel</button>
    <button data-modal-primary="true" className={`${modalStyles.btn} ${modalStyles.btnPrimary}`} onClick={() => onRename(preview)} disabled={invalid} type="button">Rename</button>
  </>}>
    <div className={modalStyles.stack}>
      <div className={styles.tabs} role="tablist" aria-label="Rename mode">
        {(['format', 'replace'] as const).map((value) => <button key={value} className={`${styles.tab} ${mode === value ? styles.tabActive : ''}`} onClick={() => setMode(value)} role="tab" aria-selected={mode === value} type="button">{value === 'format' ? 'Format' : 'Replace'}</button>)}
      </div>
      {mode === 'format' ? <>
        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Name format</label>
          <GlassInput value={pattern} onChange={(event) => setPattern(event.target.value)} />
          <span className={modalStyles.helpText}>* keeps the original name. %N, %NN, and %NNN add padded numbering.</span>
        </div>
        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Start at</label>
          <GlassInput value={String(startAt)} onChange={(event) => setStartAt(Math.max(0, Number.parseInt(event.target.value, 10) || 0))} style={{ width: 96 }} />
        </div>
      </> : <div className={modalStyles.fieldRow}>
        <GlassInput value={pattern} onChange={(event) => setPattern(event.target.value)} placeholder="Find" />
        <GlassInput value={replacement} onChange={(event) => setReplacement(event.target.value)} placeholder="Replace with" />
      </div>}
      <div className={styles.preview} aria-label="Rename preview">
        {preview.map((item, index) => <div className={styles.previewRow} key={item.root_id}>
          <span title={items[index].name}>{items[index].name}</span>
          <span title={item.name}>{item.name || 'Invalid empty name'}</span>
        </div>)}
      </div>
    </div>
  </GlassModal>;
}

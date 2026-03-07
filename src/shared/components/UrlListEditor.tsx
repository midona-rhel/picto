import { useState, useCallback, useRef, useEffect } from 'react';
import { useGlobalKeydown } from '../../shared/hooks/useGlobalKeydown';
import { IconExternalLink, IconLink, IconPlus, IconX } from '@tabler/icons-react';
import { FileController } from '../../controllers/fileController';
import { KbdTooltip } from './KbdTooltip';
import styles from './UrlListEditor.module.css';

interface UrlListEditorProps {
  urls: string[];
  onChange: (urls: string[]) => void;
  readOnly?: boolean;
}

function extractDomain(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '');
  } catch {
    return url;
  }
}

export function UrlListEditor({ urls, onChange, readOnly }: UrlListEditorProps) {
  const [open, setOpen] = useState(false);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [editingValue, setEditingValue] = useState('');
  const [showPopover, setShowPopover] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const summaryRef = useRef<HTMLDivElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const popoverTimerRef = useRef<ReturnType<typeof setTimeout>>();

  const domainSummary = urls.length > 0
    ? urls.map(extractDomain).join(', ')
    : '';

  const isTruncated = () => {
    const el = summaryRef.current;
    return el ? el.scrollWidth > el.clientWidth : false;
  };

  const handleMouseEnter = () => {
    if (open) return;
    clearTimeout(popoverTimerRef.current);
    if (isTruncated()) setShowPopover(true);
  };

  const handleMouseLeave = () => {
    popoverTimerRef.current = setTimeout(() => setShowPopover(false), 200);
  };

  const handlePopoverEnter = () => {
    clearTimeout(popoverTimerRef.current);
  };

  const handlePopoverLeave = () => {
    popoverTimerRef.current = setTimeout(() => setShowPopover(false), 200);
  };

  // Escape closes dropdown (capture phase)
  const handleEscape = useCallback((e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      setOpen(false);
      setEditingIndex(null);
      setEditingValue('');
    }
  }, []);
  useGlobalKeydown(handleEscape, open, { capture: true });

  // Click-outside closes dropdown
  useEffect(() => {
    if (!open) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false);
        setEditingIndex(null);
        setEditingValue('');
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [open]);

  const handleSave = useCallback((index: number, value: string) => {
    const trimmed = value.trim();
    setEditingIndex(null);
    setEditingValue('');
    const next = [...urls];
    if (trimmed) {
      next[index] = trimmed;
    } else {
      next.splice(index, 1);
    }
    onChange(next);
  }, [urls, onChange]);

  const handleAdd = useCallback(() => {
    const next = [...urls, ''];
    onChange(next);
    setEditingIndex(next.length - 1);
    setEditingValue('');
    if (!open) setOpen(true);
  }, [urls, onChange, open]);

  const handleRemove = useCallback((index: number) => {
    const next = urls.filter((_, i) => i !== index);
    if (editingIndex === index) {
      setEditingIndex(null);
      setEditingValue('');
    } else if (editingIndex !== null && editingIndex > index) {
      setEditingIndex(editingIndex - 1);
    }
    onChange(next);
  }, [urls, editingIndex, onChange]);

  const handleSummaryClick = () => {
    if (readOnly) return;
    setShowPopover(false);
    if (urls.length === 0) {
      handleAdd();
    } else {
      setOpen(!open);
    }
  };

  return (
    <div ref={wrapRef} className={styles.urlWrap}>
      {/* Summary row: icon | separator | domain text */}
      <div className={styles.urlSummaryRow}>
        <button
          className={styles.urlSummaryIcon}
          onClick={handleSummaryClick}
          tabIndex={-1}
        >
          <IconLink size={14} />
        </button>
        <div className={styles.urlSummarySep} />
        <div
          ref={summaryRef}
          className={styles.urlSummaryText}
          onClick={handleSummaryClick}
          onMouseEnter={handleMouseEnter}
          onMouseLeave={handleMouseLeave}
        >
          {domainSummary || <span className={styles.urlSummaryPlaceholder}>Source</span>}
        </div>
      </div>

      {/* Hover popover — full URLs, clickable to open */}
      {showPopover && !open && urls.length > 0 && (
        <div
          className={styles.urlPopover}
          onMouseEnter={handlePopoverEnter}
          onMouseLeave={handlePopoverLeave}
        >
          {urls.map((url, idx) => (
            <div
              key={idx}
              className={styles.urlPopoverRow}
              onClick={() => url.trim() && FileController.openExternalUrl(url.trim())}
            >
              {url}
            </div>
          ))}
        </div>
      )}

      {/* Dropdown panel */}
      {open && (
        <div ref={dropdownRef} className={styles.urlDropdown}>
          {urls.map((url, idx) => (
            <div key={idx} className={styles.urlRow}>
              {editingIndex === idx ? (
                <input
                  className={styles.urlEditInput}
                  autoFocus
                  value={editingValue}
                  onChange={(e) => setEditingValue(e.target.value)}
                  onBlur={() => handleSave(idx, editingValue)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') e.currentTarget.blur();
                    if (e.key === 'Escape') { setEditingIndex(null); setEditingValue(''); }
                  }}
                  placeholder="https://..."
                />
              ) : (
                <span
                  className={styles.urlText}
                  title={url}
                  onClick={readOnly ? undefined : () => { setEditingIndex(idx); setEditingValue(url); }}
                >
                  {url ? extractDomain(url) : 'https://...'}
                </span>
              )}
              {url.trim() && editingIndex !== idx && (
                <KbdTooltip label="Open link">
                  <button
                    className={styles.urlActionBtn}
                    onClick={() => FileController.openExternalUrl(url.trim())}
                  >
                    <IconExternalLink size={13} />
                  </button>
                </KbdTooltip>
              )}
              {!readOnly && (
                <KbdTooltip label="Remove">
                  <button
                    className={styles.urlRemoveBtn}
                    onClick={() => handleRemove(idx)}
                  >
                    <IconX size={13} />
                  </button>
                </KbdTooltip>
              )}
            </div>
          ))}
          {!readOnly && (
            <button className={styles.addUrlBtn} onClick={handleAdd}>
              <IconPlus size={13} />
              <span>Add URL</span>
            </button>
          )}
        </div>
      )}
    </div>
  );
}

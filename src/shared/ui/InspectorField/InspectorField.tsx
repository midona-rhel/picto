/**
 * InspectorField — view-first field with glass popover overlay.
 *
 * Field row: [content ... | sep | ⋯] — never changes shape.
 * Hover: glass popover fades in on top showing full content.
 * Edit (click ⋯): same glass popover with auto-growing textarea.
 * Name and notes behave identically — both single-line by default,
 * both auto-grow in the popover.
 */

import { useState, useRef, useEffect, useCallback } from 'react';
import { IconDots, IconLink, IconPlus, IconX, IconExternalLink } from '@tabler/icons-react';
import { openExternalUrl } from '../../../platform/api';
import styles from './InspectorField.module.css';

// ── InspectorField (name, notes, any text) ───────────────────────

interface Props {
  value: string;
  placeholder?: string;
  readOnly?: boolean;
  onCommit?: (value: string) => void;
}

export function InspectorField({ value, placeholder = '', readOnly = false, onCommit }: Props) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const [hovered, setHovered] = useState(false);
  const [isMultiRow, setIsMultiRow] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);

  useEffect(() => { setDraft(value); }, [value]);
  useEffect(() => {
    if (editing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editing]);

  // Measure popover to toggle button style
  useEffect(() => {
    const el = popoverRef.current;
    if (!el) { setIsMultiRow(false); return; }
    const check = () => setIsMultiRow(el.scrollHeight > 40);
    check();
    const ro = new ResizeObserver(check);
    ro.observe(el);
    return () => ro.disconnect();
  }, [value]);

  const commit = useCallback(() => {
    setEditing(false);
    setHovered(false);
    const trimmed = draft.trim();
    if (trimmed !== value && onCommit) onCommit(trimmed);
  }, [draft, value, onCommit]);

  const cancel = useCallback(() => {
    setDraft(value);
    setEditing(false);
    setHovered(false);
  }, [value]);

  const canEdit = !readOnly && !!onCommit;
  const showOverlay = (hovered || editing) && (!!value || editing);

  return (
    <div
      className={styles.fieldWrap}
      onMouseEnter={() => { if (!editing && value) setHovered(true); }}
      onMouseLeave={() => { if (!editing) setHovered(false); }}
    >
      <div className={styles.fieldRow}>
        <div className={styles.fieldContent}>
          {value || <span className={styles.fieldPlaceholder}>{placeholder}</span>}
        </div>
        {canEdit && (
          <>
            <div className={styles.fieldSep} />
            <button
              className={styles.fieldActionBtn}
              onClick={() => { setEditing(true); setDraft(value); }}
              type="button"
            >
              <IconDots size={14} stroke={1.5} />
            </button>
          </>
        )}
      </div>

      {showOverlay && (
        <div
          ref={popoverRef}
          className={`${styles.popover} ${!isMultiRow ? styles.popoverSingleRow : ''}`}
        >
          <div className={styles.popoverBody}>
            {editing ? (
              <textarea
                ref={inputRef}
                className={styles.popoverInput}
                value={draft}
                rows={1}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Escape') cancel();
                  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); commit(); }
                }}
                onBlur={commit}
              />
            ) : (
              <div className={styles.popoverText}>{value}</div>
            )}
          </div>
          {canEdit && (
            <button
              className={styles.popoverActionBtn}
              onClick={() => { if (editing) commit(); else { setEditing(true); setDraft(value); } }}
              type="button"
            >
              <IconDots size={14} stroke={1.5} />
            </button>
          )}
        </div>
      )}
    </div>
  );
}

// ── InspectorSourceField (URLs) ──────────────────────────────────

interface SourceFieldProps {
  urls: string[];
  onChange?: (urls: string[]) => void;
  readOnly?: boolean;
}

function extractDomain(url: string): string {
  try { return new URL(url).hostname.replace(/^www\./, ''); } catch { return url; }
}

export function InspectorSourceField({ urls, onChange, readOnly = false }: SourceFieldProps) {
  const [open, setOpen] = useState(false);
  const [editIdx, setEditIdx] = useState<number | null>(null);
  const [editVal, setEditVal] = useState('');
  const [hovered, setHovered] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const hideTimer = useRef<ReturnType<typeof setTimeout>>();

  const domainSummary = urls.length > 0 ? urls.map(extractDomain).join(', ') : '';
  const canEdit = !readOnly && !!onChange;
  const showPopover = hovered && !open && urls.length > 0;

  const clearHide = () => clearTimeout(hideTimer.current);
  const scheduleHide = (ms: number) => {
    hideTimer.current = setTimeout(() => setHovered(false), ms);
  };

  useEffect(() => () => clearTimeout(hideTimer.current), []);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false); setEditIdx(null); setEditVal('');
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  const handleSave = useCallback((index: number, val: string) => {
    const trimmed = val.trim();
    setEditIdx(null); setEditVal('');
    if (!onChange) return;
    if (index >= urls.length) {
      // New entry — only persist if non-empty
      if (trimmed) onChange([...urls, trimmed]);
    } else {
      const next = [...urls];
      if (trimmed) next[index] = trimmed; else next.splice(index, 1);
      onChange(next);
    }
  }, [urls, onChange]);

  const handleAdd = useCallback(() => {
    if (!onChange) return;
    // Don't persist yet — just open an empty edit row.
    // The new URL is committed only when the user types and blurs/enters.
    setEditIdx(urls.length);
    setEditVal('');
    if (!open) setOpen(true);
  }, [urls, onChange, open]);

  const handleRemove = useCallback((index: number) => {
    if (!onChange) return;
    onChange(urls.filter((_, i) => i !== index));
    if (editIdx === index) { setEditIdx(null); setEditVal(''); }
  }, [urls, editIdx, onChange]);

  const handleBtnClick = () => {
    if (!canEdit) return;
    setHovered(false);
    if (urls.length === 0) handleAdd(); else setOpen(!open);
  };

  return (
    <div ref={wrapRef} className={styles.fieldWrap}>
      <div
        className={styles.fieldRow}
        onMouseEnter={() => { if (!open) { clearHide(); if (urls.length > 0) setHovered(true); } }}
        onMouseLeave={() => scheduleHide(400)}
      >
        <div className={styles.fieldContent} onClick={handleBtnClick}>
          {domainSummary || <span className={styles.fieldPlaceholder}>Source</span>}
        </div>
        <div className={styles.fieldSep} />
        <button className={styles.fieldActionBtn} onClick={handleBtnClick} type="button">
          <IconLink size={14} stroke={1.5} />
        </button>
      </div>

      {showPopover && (
        <div
          className={styles.popover}
          onMouseEnter={clearHide}
          onMouseLeave={() => scheduleHide(200)}
        >
          <div className={styles.popoverBody}>
            {urls.map((url, i) => (
              <a
                key={i}
                className={styles.popoverLink}
                href={url}
                onClick={(e) => { e.preventDefault(); void openExternalUrl(url); }}
              >
                {url}
              </a>
            ))}
          </div>
          <button className={styles.popoverActionBtn} onClick={handleBtnClick} type="button">
            <IconLink size={14} stroke={1.5} />
          </button>
        </div>
      )}

      {open && (
        <div className={styles.urlDropdown}>
          {urls.map((url, idx) => (
            <div key={idx} className={styles.urlRow}>
              {editIdx === idx ? (
                <input
                  className={styles.urlEditInput}
                  autoFocus
                  value={editVal}
                  onChange={(e) => setEditVal(e.target.value)}
                  onBlur={() => handleSave(idx, editVal)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') e.currentTarget.blur();
                    if (e.key === 'Escape') { setEditIdx(null); setEditVal(''); }
                  }}
                  placeholder="https://..."
                />
              ) : (
                <span
                  className={styles.urlText}
                  title={url}
                  onClick={canEdit ? () => { setEditIdx(idx); setEditVal(url); } : undefined}
                >
                  {url ? extractDomain(url) : 'https://...'}
                </span>
              )}
              {url.trim() && editIdx !== idx && (
                <button
                  className={styles.urlActionBtn}
                  onClick={() => { void openExternalUrl(url); }}
                  type="button" title="Open link"
                >
                  <IconExternalLink size={13} stroke={1.5} />
                </button>
              )}
              {canEdit && (
                <button
                  className={styles.urlActionBtn}
                  onClick={() => handleRemove(idx)}
                  type="button" title="Remove"
                >
                  <IconX size={13} stroke={1.5} />
                </button>
              )}
            </div>
          ))}
          {editIdx === urls.length && (
            <div className={styles.urlRow}>
              <input
                className={styles.urlEditInput}
                autoFocus
                value={editVal}
                onChange={(e) => setEditVal(e.target.value)}
                onBlur={() => handleSave(urls.length, editVal)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') e.currentTarget.blur();
                  if (e.key === 'Escape') { setEditIdx(null); setEditVal(''); }
                }}
                placeholder="https://..."
              />
            </div>
          )}
          {canEdit && (
            <button className={styles.addUrlBtn} onClick={handleAdd} type="button">
              <IconPlus size={13} stroke={1.5} />
              <span>Add URL</span>
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * InspectorField — view-first field with glass popover overlay.
 *
 * Field row: [content ... | sep | ⋯] — never changes shape.
 * Hover: glass popover fades in on top showing full content.
 * Edit (click ⋯): same glass popover with auto-growing textarea.
 * Name and notes behave identically — both single-line by default,
 * both auto-grow in the popover.
 */

import { createContext, useState, useRef, useEffect, useCallback, useContext, useId, type ReactNode } from 'react';
import { IconDots, IconLink, IconPlus, IconX, IconExternalLink } from '@tabler/icons-react';
import { shellController } from '../../../controllers/shellController';
import { KbdTooltip } from '../KbdTooltip';
import styles from './InspectorField.module.css';

type HoverGroup = {
  activeId: string | null;
  activate: (id: string) => void;
  deactivate: (id: string, delay?: number) => void;
  lock: (id: string) => void;
  unlock: (id: string) => void;
};

const InspectorFieldHoverContext = createContext<HoverGroup | null>(null);

export function InspectorFieldGroup({ children }: { children: ReactNode }) {
  const [activeId, setActiveId] = useState<string | null>(null);
  const lockedId = useRef<string | null>(null);
  const hideTimer = useRef<ReturnType<typeof setTimeout>>();
  const clearHide = useCallback(() => clearTimeout(hideTimer.current), []);

  useEffect(() => clearHide, [clearHide]);

  const value: HoverGroup = {
    activeId,
    activate: (id) => {
      if (lockedId.current && lockedId.current !== id) return;
      clearHide();
      setActiveId(id);
    },
    deactivate: (id, delay = 140) => {
      if (lockedId.current === id) return;
      clearHide();
      hideTimer.current = setTimeout(() => {
        setActiveId((current) => current === id ? null : current);
      }, delay);
    },
    lock: (id) => {
      clearHide();
      lockedId.current = id;
      setActiveId(id);
    },
    unlock: (id) => {
      if (lockedId.current !== id) return;
      lockedId.current = null;
      setActiveId((current) => current === id ? null : current);
    },
  };

  return <InspectorFieldHoverContext.Provider value={value}>{children}</InspectorFieldHoverContext.Provider>;
}

function useFieldHover() {
  const group = useContext(InspectorFieldHoverContext);
  const id = useId();
  return {
    active: group?.activeId === id,
    activate: () => group?.activate(id),
    deactivate: (delay?: number) => group?.deactivate(id, delay),
    lock: () => group?.lock(id),
    unlock: () => group?.unlock(id),
  };
}

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
  const hover = useFieldHover();
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
    hover.unlock();
    const trimmed = draft.trim();
    if (trimmed !== value && onCommit) onCommit(trimmed);
  }, [draft, value, onCommit, hover]);

  const cancel = useCallback(() => {
    setDraft(value);
    setEditing(false);
    hover.unlock();
  }, [value, hover]);

  const canEdit = !readOnly && !!onCommit;
  const showOverlay = (hover.active || editing) && (!!value || editing);

  return (
    <div
      className={styles.fieldWrap}
      onMouseEnter={() => { if (!editing && value) hover.activate(); }}
      onMouseLeave={() => { if (!editing) hover.deactivate(); }}
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
              onClick={() => { hover.lock(); setEditing(true); setDraft(value); }}
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
          data-inspector-field-popover=""
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
              onClick={() => { if (editing) commit(); else { hover.lock(); setEditing(true); setDraft(value); } }}
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
  unavailable?: boolean;
}

function extractDomain(url: string): string {
  try { return new URL(url).hostname.replace(/^www\./, ''); } catch { return url; }
}

export function InspectorSourceField({ urls, onChange, readOnly = false, unavailable = false }: SourceFieldProps) {
  const hover = useFieldHover();
  const [open, setOpen] = useState(false);
  const [editIdx, setEditIdx] = useState<number | null>(null);
  const [editVal, setEditVal] = useState('');
  const wrapRef = useRef<HTMLDivElement>(null);

  const domainSummary = unavailable ? '—' : urls.length > 0 ? urls.map(extractDomain).join(', ') : '';
  const canEdit = !unavailable && !readOnly && !!onChange;
  const showPopover = !unavailable && hover.active && !open && urls.length > 0;

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false); setEditIdx(null); setEditVal('');
        hover.unlock();
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open, hover]);

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
    if (open) {
      setOpen(false);
      hover.unlock();
    } else {
      hover.lock();
      if (urls.length === 0) handleAdd(); else setOpen(true);
    }
  };

  return (
    <div ref={wrapRef} className={styles.fieldWrap}>
      <div
        className={styles.fieldRow}
        onMouseEnter={() => { if (!unavailable && !open && urls.length > 0) hover.activate(); }}
        onMouseLeave={() => hover.deactivate()}
      >
        <div className={styles.fieldContent} onClick={handleBtnClick}>
          {domainSummary || <span className={styles.fieldPlaceholder}>Source</span>}
        </div>
        {!unavailable && <>
          <div className={styles.fieldSep} />
          <button className={styles.fieldActionBtn} onClick={handleBtnClick} type="button">
            <IconLink size={14} stroke={1.5} />
          </button>
        </>}
      </div>

      {showPopover && (
        <div
          data-inspector-field-popover=""
          className={styles.popover}
          onMouseEnter={hover.activate}
          onMouseLeave={() => hover.deactivate()}
        >
          <div className={styles.popoverBody}>
            {urls.map((url, i) => (
              <a
                key={i}
                className={styles.popoverLink}
                href={url}
                onClick={(e) => { e.preventDefault(); void shellController.openExternalUrl(url); }}
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
                <KbdTooltip label="Open link">
                  <button
                    className={styles.urlActionBtn}
                    onClick={() => { void shellController.openExternalUrl(url); }}
                    type="button" aria-label="Open link"
                  >
                    <IconExternalLink size={13} stroke={1.5} />
                  </button>
                </KbdTooltip>
              )}
              {canEdit && (
                <KbdTooltip label="Remove">
                  <button
                    className={styles.urlActionBtn}
                    onClick={() => handleRemove(idx)}
                    type="button" aria-label="Remove"
                  >
                    <IconX size={13} stroke={1.5} />
                  </button>
                </KbdTooltip>
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

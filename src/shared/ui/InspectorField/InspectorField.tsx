import { createContext, useContext, useId, useState, useRef, useEffect, useCallback, useLayoutEffect, type ReactNode } from 'react';
import { IconEdit, IconLink, IconPlus, IconTrash } from '@tabler/icons-react';
import { shellController } from '../../../controllers/shellController';
import { KbdTooltip } from '../KbdTooltip';
import styles from './InspectorField.module.css';
import { t } from '../../../i18n';

type HoverGroup = {
  activeId: string | null;
  setActiveId: (id: string | null) => void;
};

const InspectorFieldHoverContext = createContext<HoverGroup | null>(null);

export function InspectorFieldGroup({ children }: { children: ReactNode }) {
  const [activeId, setActiveId] = useState<string | null>(null);
  return (
    <InspectorFieldHoverContext.Provider value={{ activeId, setActiveId }}>
      {children}
    </InspectorFieldHoverContext.Provider>
  );
}

function useFieldHover() {
  const id = useId();
  const group = useContext(InspectorFieldHoverContext);
  return {
    active: group?.activeId === id,
    activate: () => group?.setActiveId(id),
    deactivate: () => {
      if (group?.activeId === id) group.setActiveId(null);
    },
  };
}

interface Props {
  value: string;
  placeholder?: string;
  readOnly?: boolean;
  onCommit?: (value: string) => void;
}

export function InspectorField({ value, placeholder = '', readOnly = false, onCommit }: Props) {
  const [focused, setFocused] = useState(false);
  const [overflowing, setOverflowing] = useState(false);
  const fieldRef = useRef<HTMLDivElement>(null);
  const backingRef = useRef<HTMLDivElement>(null);
  const measureRef = useRef<HTMLDivElement>(null);
  const canEdit = !readOnly && Boolean(onCommit);
  const hover = useFieldHover();
  const expanded = overflowing && focused;

  const measureOverflow = useCallback(() => {
    const measure = measureRef.current;
    if (!measure) return;
    setOverflowing(measure.scrollHeight > 32 || measure.scrollWidth > measure.clientWidth + 1);
  }, []);

  useLayoutEffect(() => {
    if (!focused && fieldRef.current?.textContent !== value) fieldRef.current!.textContent = value;
    if (measureRef.current) measureRef.current.dataset.value = value || placeholder;
    measureOverflow();
  }, [focused, measureOverflow, placeholder, value]);

  useEffect(() => {
    const wrapper = fieldRef.current?.parentElement;
    if (!wrapper || typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(measureOverflow);
    observer.observe(wrapper);
    return () => observer.disconnect();
  }, [measureOverflow]);

  const commit = useCallback(() => {
    const trimmed = (fieldRef.current?.textContent ?? '').trim();
    if (fieldRef.current) fieldRef.current.textContent = trimmed;
    if (backingRef.current) backingRef.current.textContent = trimmed || placeholder;
    if (measureRef.current) measureRef.current.dataset.value = trimmed || placeholder;
    measureOverflow();
    if (trimmed !== value) onCommit?.(trimmed);
  }, [measureOverflow, onCommit, placeholder, value]);

  const cancel = useCallback(() => {
    if (fieldRef.current) fieldRef.current.textContent = value;
    if (backingRef.current) backingRef.current.textContent = value || placeholder;
    if (measureRef.current) measureRef.current.dataset.value = value || placeholder;
    measureOverflow();
  }, [measureOverflow, placeholder, value]);

  return (
    <div
      className={styles.fieldWrap}
      onMouseLeave={() => { if (!focused) hover.deactivate(); }}
    >
      {expanded && (
        <div
          ref={backingRef}
          className={styles.fieldExpansionBackdrop}
          data-inspector-field-backdrop=""
          aria-hidden="true"
        >
          {value || placeholder}
        </div>
      )}
      <div
        ref={fieldRef}
        className={`${styles.fieldControl} ${expanded ? styles.fieldControlExpanded : ''} ${readOnly ? styles.fieldDisabled : ''}`}
        contentEditable={canEdit ? 'plaintext-only' : false}
        suppressContentEditableWarning
        role="textbox"
        aria-label={placeholder}
        aria-readonly={!canEdit}
        data-placeholder={placeholder}
        data-inspector-field-popover={expanded ? '' : undefined}
        data-inspector-field-expanded={expanded ? '' : undefined}
        tabIndex={canEdit ? 0 : undefined}
        onFocus={canEdit ? () => { hover.activate(); setFocused(true); } : undefined}
        onInput={canEdit ? (event) => {
          const nextValue = event.currentTarget.textContent ?? '';
          if (backingRef.current) backingRef.current.textContent = nextValue || placeholder;
          if (measureRef.current) measureRef.current.dataset.value = nextValue;
          measureOverflow();
        } : undefined}
        onBlur={canEdit ? () => { commit(); setFocused(false); } : undefined}
        onKeyDown={canEdit ? (event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            cancel();
            event.currentTarget.blur();
          } else if (event.key === 'Enter' && !event.shiftKey) {
            event.preventDefault();
            event.currentTarget.blur();
          }
        } : undefined}
      >
        {value}
      </div>
      <div
        ref={measureRef}
        className={styles.fieldMeasure}
        data-inspector-field-measure=""
        data-value={value || placeholder}
        aria-hidden="true"
      />
    </div>
  );
}

interface SourceFieldProps {
  urls: string[];
  onChange?: (urls: string[]) => void;
  readOnly?: boolean;
  unavailable?: boolean;
}

function splitUrl(url: string): { domain: string; remainder: string } {
  try {
    const parsed = new URL(url);
    return {
      domain: parsed.hostname.replace(/^www\./, ''),
      remainder: `${parsed.pathname === '/' ? '' : parsed.pathname}${parsed.search}${parsed.hash}`,
    };
  } catch {
    return { domain: url, remainder: '' };
  }
}

function UrlLabel({ url }: { url: string }) {
  const { domain, remainder } = splitUrl(url);
  return (
    <span className={styles.urlLabel}>
      <span className={styles.urlDomain}>{domain}</span>
      {remainder && <span className={styles.urlRemainder}>{remainder}</span>}
    </span>
  );
}

export function InspectorSourceField({ urls, onChange, readOnly = false, unavailable = false }: SourceFieldProps) {
  const [open, setOpen] = useState(false);
  const [editIdx, setEditIdx] = useState<number | null>(null);
  const [editVal, setEditVal] = useState('');
  const wrapRef = useRef<HTMLDivElement>(null);
  const primaryUrl = urls[0] ?? '';
  const canEdit = !unavailable && !readOnly && Boolean(onChange);

  useEffect(() => {
    if (!open) return;
    const handler = (event: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(event.target as Node)) {
        setOpen(false);
        setEditIdx(null);
        setEditVal('');
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  const handleSave = useCallback((index: number, nextValue: string) => {
    const trimmed = nextValue.trim();
    setEditIdx(null);
    setEditVal('');
    if (!onChange) return;
    if (index >= urls.length) {
      if (trimmed) onChange([...urls, trimmed]);
      return;
    }
    const next = [...urls];
    if (trimmed) next[index] = trimmed;
    else next.splice(index, 1);
    onChange(next);
  }, [onChange, urls]);

  const handleAdd = useCallback(() => {
    if (!onChange) return;
    setOpen(true);
    setEditIdx(urls.length);
    setEditVal('');
  }, [onChange, urls.length]);

  const handleRemove = useCallback((index: number) => {
    onChange?.(urls.filter((_, itemIndex) => itemIndex !== index));
    if (editIdx === index) { setEditIdx(null); setEditVal(''); }
  }, [editIdx, onChange, urls]);

  const toggleOpen = () => {
    if (!canEdit) return;
    if (urls.length === 0) handleAdd();
    else setOpen((current) => !current);
  };

  return (
    <div
      ref={wrapRef}
      className={styles.fieldWrap}
    >
      <div className={`${styles.sourceControl} ${open ? styles.sourceControlOpen : ''} ${unavailable ? styles.fieldDisabled : ''}`}>
        <button
          className={styles.sourceValue}
          onClick={() => { if (primaryUrl) void shellController.openExternalUrl(primaryUrl); else handleAdd(); }}
          type="button"
          disabled={unavailable || (!primaryUrl && !canEdit)}
          aria-label={primaryUrl ? t("Open {value0}", { value0: primaryUrl }) : t("Add source URL")}
        >
          {unavailable
            ? '—'
            : primaryUrl
              ? <UrlLabel url={primaryUrl} />
              : <span className={styles.fieldPlaceholder}>{t("http://")}</span>}
        </button>
        {urls.length > 1 && !unavailable && (
          <button
            className={styles.urlCount}
            onClick={toggleOpen}
            type="button"
            disabled={!canEdit}
            aria-expanded={open}
            aria-label={t("Show all {value0} source URLs", { value0: urls.length })}
          >
            +{urls.length - 1}
          </button>
        )}
        {!unavailable && (
          <button className={styles.sourceAction} onClick={toggleOpen} type="button" disabled={!canEdit} aria-expanded={open} aria-label={t("Manage source URLs")}>
            <IconLink size={14} stroke={1.5} />
          </button>
        )}
      </div>

      {open && (
        <div className={styles.urlDropdown}>
          {urls.map((url, index) => (
            <div key={`${url}-${index}`} className={styles.urlRow}>
              {editIdx === index ? (
                <input
                  className={styles.urlEditInput}
                  autoFocus
                  value={editVal}
                  onChange={(event) => setEditVal(event.target.value)}
                  onBlur={() => handleSave(index, editVal)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') event.currentTarget.blur();
                    if (event.key === 'Escape') { setEditIdx(null); setEditVal(''); }
                  }}
                  placeholder={t("https://...")}
                />
              ) : (
                <button
                  className={styles.urlText}
                  onClick={() => { void shellController.openExternalUrl(url); }}
                  type="button"
                  aria-label={t("Open {value0}", { value0: url })}
                >
                  <UrlLabel url={url} />
                </button>
              )}
              <KbdTooltip label={t("Edit URL")}>
                <button className={styles.urlActionBtn} onClick={() => { setEditIdx(index); setEditVal(url); }} type="button" aria-label={t("Edit URL")}>
                  <IconEdit size={13} stroke={1.5} />
                </button>
              </KbdTooltip>
              <KbdTooltip label={t("Delete URL")}>
                <button className={styles.urlActionBtn} onClick={() => handleRemove(index)} type="button" aria-label={t("Delete URL")}>
                  <IconTrash size={13} stroke={1.5} />
                </button>
              </KbdTooltip>
            </div>
          ))}
          {editIdx === urls.length && (
            <div className={styles.urlRow}>
              <input
                className={styles.urlEditInput}
                autoFocus
                value={editVal}
                onChange={(event) => setEditVal(event.target.value)}
                onBlur={() => handleSave(urls.length, editVal)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') event.currentTarget.blur();
                  if (event.key === 'Escape') { setEditIdx(null); setEditVal(''); }
                }}
                placeholder={t("https://...")}
              />
            </div>
          )}
          <button className={styles.addUrlBtn} onClick={handleAdd} type="button">
            <IconPlus size={13} stroke={1.5} />
            <span>{t("Add URL")}</span>
          </button>
        </div>
      )}
    </div>
  );
}

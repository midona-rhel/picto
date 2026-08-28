import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { tagsController } from '../../../controllers/tagsController';
import type { CanonicalTagRecord } from '../../types/canonical';
import { TagChip } from '../TagChip/TagChip';
import styles from './TagTokenInput.module.css';

function splitTag(tag: string): { namespace: string; subtag: string } {
  const separator = tag.indexOf(':');
  return separator < 0
    ? { namespace: 'general', subtag: tag }
    : { namespace: tag.slice(0, separator), subtag: tag.slice(separator + 1) };
}

function formatTag(tag: CanonicalTagRecord): string {
  return !tag.namespace || tag.namespace === 'general'
    ? tag.subname
    : `${tag.namespace}:${tag.subname}`;
}

export function TagTokenInput({
  values,
  onChange,
  autoFocus = false,
  compact = false,
  ariaLabel = 'Tags',
}: {
  values: string[];
  onChange: (values: string[]) => void;
  autoFocus?: boolean;
  compact?: boolean;
  ariaLabel?: string;
}) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<CanonicalTagRecord[]>([]);
  const [open, setOpen] = useState(false);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (autoFocus) inputRef.current?.focus();
  }, [autoFocus]);

  useEffect(() => () => {
    if (timerRef.current) clearTimeout(timerRef.current);
  }, []);

  const search = (value: string) => {
    if (timerRef.current) clearTimeout(timerRef.current);
    if (!value.trim()) {
      setResults([]);
      setOpen(false);
      return;
    }
    timerRef.current = setTimeout(() => {
      void tagsController.getPaginated({ search: value, limit: 12 }).then(({ tags }) => {
        const existing = new Set(values);
        const next = tags.filter((item) => !existing.has(formatTag(item))).slice(0, 8);
        setResults(next);
        setOpen(next.length > 0);
        setSelectedIndex(0);
      }).catch(() => {
        setResults([]);
        setOpen(false);
      });
    }, 120);
  };

  const addTag = (tag: string) => {
    const normalized = tag.trim();
    if (normalized && !values.includes(normalized)) onChange([...values, normalized]);
    setQuery('');
    setResults([]);
    setOpen(false);
    inputRef.current?.focus();
  };

  const removeTag = (tag: string) => onChange(values.filter((value) => value !== tag));
  const rect = wrapperRef.current?.getBoundingClientRect();

  return (
    <div ref={wrapperRef} className={`${styles.wrapper} ${compact ? styles.compact : ''}`}>
      <div className={styles.inputSurface} onClick={() => inputRef.current?.focus()}>
        {values.map((tag) => {
          const { namespace, subtag } = splitTag(tag);
          return (
            <TagChip
              key={tag}
              namespace={namespace}
              subtag={subtag}
              onRemove={() => removeTag(tag)}
            />
          );
        })}
        <input
          ref={inputRef}
          aria-label={ariaLabel}
          className={styles.input}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            search(event.target.value);
          }}
          onFocus={() => { if (results.length > 0) setOpen(true); }}
          onBlur={() => setTimeout(() => setOpen(false), 150)}
          onKeyDown={(event) => {
            if (event.key === 'Backspace' && !query && values.length > 0) {
              removeTag(values[values.length - 1]);
              return;
            }
            if (open && results.length > 0) {
              if (event.key === 'ArrowDown') {
                event.preventDefault();
                setSelectedIndex((index) => Math.min(index + 1, results.length - 1));
              } else if (event.key === 'ArrowUp') {
                event.preventDefault();
                setSelectedIndex((index) => Math.max(index - 1, 0));
              } else if (event.key === 'Enter' || event.key === 'Tab') {
                event.preventDefault();
                addTag(formatTag(results[selectedIndex]));
              } else if (event.key === 'Escape') {
                setOpen(false);
              }
            } else if (event.key === 'Enter' && query.trim()) {
              event.preventDefault();
              addTag(query);
            }
          }}
          placeholder={values.length === 0 ? 'Search tags...' : ''}
        />
      </div>
      {open && rect && createPortal(
        <div
          className={styles.results}
          style={{ top: rect.bottom + 2, left: rect.left, width: rect.width }}
        >
          {results.map((result, index) => (
            <button
              key={`${result.namespace}:${result.subname}`}
              className={`${styles.result} ${index === selectedIndex ? styles.resultSelected : ''}`}
              onMouseDown={(event) => {
                event.preventDefault();
                addTag(formatTag(result));
              }}
              type="button"
            >
              <TagChip namespace={result.namespace} subtag={result.subname} />
            </button>
          ))}
        </div>,
        document.body,
      )}
    </div>
  );
}

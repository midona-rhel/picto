import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { GlassInput } from '../../../shared/ui/GlassInput/GlassInput';
import { rectToCSS } from '../../../shared/lib/zoomCompensation';
import { subscriptionsController } from '../../../controllers/subscriptionsController';
import type { TagSuggestion } from '../../../platform/subscriptionApi';
import styles from '../SubscriptionsScreen.module.css';

const DEBOUNCE_MS = 250;

function formatCount(count: number): string {
  if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(1)}M`;
  if (count >= 1_000) return `${(count / 1_000).toFixed(0)}k`;
  return String(count);
}

function currentWordFrom(text: string, caret: number): string {
  const head = text.slice(0, caret);
  const start = head.lastIndexOf(' ') + 1;
  return head.slice(start);
}

/**
 * Query input with booru tag autocomplete. Suggestions complete the word
 * under the caret, so multi-tag queries ("1girl sol|") work naturally.
 * The dropdown renders in a portal above everything (modals included) so
 * it never stretches its scroll container. Sites without autocomplete just
 * get a plain input.
 */
export function TagAutocompleteInput({
  siteId,
  value,
  onChange,
  onSubmit,
  placeholder,
  autoFocus,
}: {
  siteId: string;
  value: string;
  onChange: (next: string) => void;
  onSubmit?: () => void;
  placeholder?: string;
  autoFocus?: boolean;
}) {
  const [suggestions, setSuggestions] = useState<TagSuggestion[]>([]);
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const [anchor, setAnchor] = useState<{ left: number; top: number; width: number } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const requestSeqRef = useRef(0);

  const fetchSuggestions = useCallback((prefix: string) => {
    const seq = ++requestSeqRef.current;
    subscriptionsController
      .suggestSiteTags(siteId, prefix)
      .then((result) => {
        if (seq !== requestSeqRef.current) return;
        setSuggestions(result);
        setActiveIndex(0);
        setOpen(result.length > 0);
      })
      .catch(() => {
        if (seq === requestSeqRef.current) setOpen(false);
      });
  }, [siteId]);

  useEffect(() => () => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
  }, []);

  // Anchor the portal dropdown under the input — fixed positioning needs
  // zoom-compensated CSS coordinates. Track scroll/resize while open.
  useLayoutEffect(() => {
    if (!open) return;
    const update = () => {
      const el = inputRef.current;
      if (!el) return;
      const css = rectToCSS(el.getBoundingClientRect());
      setAnchor({ left: css.left, top: css.bottom + 4, width: css.width });
    };
    update();
    window.addEventListener('scroll', update, true);
    window.addEventListener('resize', update);
    return () => {
      window.removeEventListener('scroll', update, true);
      window.removeEventListener('resize', update);
    };
  }, [open]);

  const handleChange = (next: string) => {
    onChange(next);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      const word = currentWordFrom(next, inputRef.current?.selectionStart ?? next.length);
      if (word.length >= 2) fetchSuggestions(word);
      else setOpen(false);
    }, DEBOUNCE_MS);
  };

  const applySuggestion = (suggestion: TagSuggestion) => {
    const caret = inputRef.current?.selectionStart ?? value.length;
    const head = value.slice(0, caret);
    const tail = value.slice(caret);
    const start = head.lastIndexOf(' ') + 1;
    const next = `${head.slice(0, start)}${suggestion.name}${tail.startsWith(' ') || tail === '' ? '' : ' '}${tail}`;
    onChange(next);
    setOpen(false);
    inputRef.current?.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (open && suggestions.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setActiveIndex((i) => (i + 1) % suggestions.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setActiveIndex((i) => (i - 1 + suggestions.length) % suggestions.length);
        return;
      }
      if (e.key === 'Tab' || e.key === 'Enter') {
        e.preventDefault();
        applySuggestion(suggestions[activeIndex]);
        return;
      }
      if (e.key === 'Escape') {
        setOpen(false);
        return;
      }
    }
    if (e.key === 'Enter') onSubmit?.();
  };

  return (
    <div className={styles.acWrap}>
      <GlassInput
        ref={inputRef}
        value={value}
        placeholder={placeholder}
        autoFocus={autoFocus}
        onChange={(e) => handleChange(e.target.value)}
        onKeyDown={handleKeyDown}
        onBlur={() => setTimeout(() => setOpen(false), 120)}
        spellCheck={false}
      />
      {open && anchor && createPortal(
        <div
          className={styles.acDropdown}
          style={{ left: anchor.left, top: anchor.top, width: anchor.width }}
        >
          {suggestions.map((suggestion, index) => (
            <button
              key={suggestion.name}
              type="button"
              className={`${styles.acItem} ${index === activeIndex ? styles.acItemActive : ''}`.trim()}
              onMouseDown={(e) => {
                e.preventDefault();
                applySuggestion(suggestion);
              }}
              onMouseEnter={() => setActiveIndex(index)}
            >
              <span className={styles.acItemName}>{suggestion.name}</span>
              {suggestion.category && suggestion.category !== 'general' && (
                <span className={styles.acItemCategory}>{suggestion.category}</span>
              )}
              {suggestion.post_count != null && (
                <span className={styles.acItemCount}>{formatCount(suggestion.post_count)}</span>
              )}
            </button>
          ))}
        </div>,
        document.body,
      )}
    </div>
  );
}

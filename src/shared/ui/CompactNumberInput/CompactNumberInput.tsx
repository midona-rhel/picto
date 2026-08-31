import { useEffect, useState } from 'react';
import { IconChevronDown, IconChevronUp } from '@tabler/icons-react';
import { GlassInput } from '../GlassInput/GlassInput';
import styles from './CompactNumberInput.module.css';
import { t } from '../../../i18n';

export function CompactNumberInput({
  value,
  min,
  max,
  label,
  disabled = false,
  commitOnChange = false,
  onCommit,
}: {
  value: number;
  min: number;
  max: number;
  label: string;
  disabled?: boolean;
  commitOnChange?: boolean;
  onCommit: (value: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);

  const parse = (text: string) => {
    const parsed = Number.parseInt(text, 10);
    return Number.isInteger(parsed) && parsed >= min && parsed <= max ? parsed : null;
  };
  const commit = () => {
    const next = parse(draft);
    if (next === null) setDraft(String(value));
    else if (next !== value) onCommit(next);
  };
  const step = (delta: number) => {
    const next = Math.min(max, Math.max(min, (parse(draft) ?? value) + delta));
    setDraft(String(next));
    if (next !== value) onCommit(next);
  };

  return (
    <span className={styles.root}>
      <GlassInput
        type="number"
        min={min}
        max={max}
        step={1}
        value={draft}
        disabled={disabled}
        aria-label={label}
        onChange={(event) => {
          setDraft(event.target.value);
          if (commitOnChange) {
            const next = parse(event.target.value);
            if (next !== null && next !== value) onCommit(next);
          }
        }}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur();
          if (event.key === 'Escape') {
            setDraft(String(value));
            event.currentTarget.blur();
          }
        }}
      />
      <span className={styles.stepper}>
        <button type="button" aria-label={t("Increase {value0}", { value0: label.toLowerCase() })} disabled={disabled}
          onMouseDown={(event) => event.preventDefault()} onClick={() => step(1)}>
          <IconChevronUp size={11} stroke={2} />
        </button>
        <button type="button" aria-label={t("Decrease {value0}", { value0: label.toLowerCase() })} disabled={disabled}
          onMouseDown={(event) => event.preventDefault()} onClick={() => step(-1)}>
          <IconChevronDown size={11} stroke={2} />
        </button>
      </span>
    </span>
  );
}

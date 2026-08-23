import { useRef } from 'react';
import { GlassInput } from '../../../shared/ui/GlassInput/GlassInput';
import styles from '../SubscriptionsScreen.module.css';

/**
 * Source query input. Source-specific suggestion services are intentionally not
 * part of the subscription contract; every source accepts its native query text.
 */
export function TagAutocompleteInput({
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
  const inputRef = useRef<HTMLInputElement>(null);
  return (
    <div className={styles.acWrap}>
      <GlassInput
        ref={inputRef}
        value={value}
        placeholder={placeholder}
        autoFocus={autoFocus}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter') onSubmit?.();
        }}
        spellCheck={false}
      />
    </div>
  );
}

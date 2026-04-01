/**
 * GlassInput — styled input for modals and portals.
 * Two variants: standard text input and search input with icon.
 */

import { forwardRef, type InputHTMLAttributes } from 'react';
import { IconSearch } from '@tabler/icons-react';
import styles from './GlassInput.module.css';

export interface GlassInputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'className'> {
  /** Add search icon on the left. */
  search?: boolean;
}

export const GlassInput = forwardRef<HTMLInputElement, GlassInputProps>(
  ({ search, readOnly, ...props }, ref) => {
    if (search) {
      return (
        <div className={styles.searchWrap}>
          <IconSearch size={14} className={styles.searchIcon} />
          <input ref={ref} className={styles.searchInput} {...props} />
        </div>
      );
    }
    return (
      <input
        ref={ref}
        className={`${styles.input} ${readOnly ? styles.inputReadOnly : ''}`}
        readOnly={readOnly}
        {...props}
      />
    );
  },
);
GlassInput.displayName = 'GlassInput';

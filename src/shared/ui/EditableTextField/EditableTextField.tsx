/**
 * EditableTextField — view-first inline editor.
 *
 * View mode: shows text as a styled pill. Click to edit.
 * Edit mode: inline input. Enter to commit, Escape to cancel.
 */

import { useState, useRef, useEffect } from 'react';
import styles from './EditableTextField.module.css';

interface Props {
  value: string;
  onCommit: (value: string) => void;
  placeholder?: string;
  readOnly?: boolean;
}

export function EditableTextField({ value, onCommit, placeholder = '', readOnly = false }: Props) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => { setDraft(value); }, [value]);
  useEffect(() => { if (editing) inputRef.current?.focus(); }, [editing]);

  const commit = () => {
    const trimmed = draft.trim();
    setEditing(false);
    if (trimmed !== value) onCommit(trimmed);
  };

  const cancel = () => {
    setDraft(value);
    setEditing(false);
  };

  if (readOnly || !editing) {
    return (
      <div
        className={styles.field}
        onClick={() => !readOnly && setEditing(true)}
        role={readOnly ? undefined : 'button'}
        tabIndex={readOnly ? undefined : 0}
        onKeyDown={(e) => { if (!readOnly && e.key === 'Enter') setEditing(true); }}
      >
        {value || <span className={styles.placeholder}>{placeholder}</span>}
      </div>
    );
  }

  return (
    <input
      ref={inputRef}
      className={styles.input}
      value={draft}
      placeholder={placeholder}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') commit();
        if (e.key === 'Escape') cancel();
      }}
      onBlur={commit}
    />
  );
}

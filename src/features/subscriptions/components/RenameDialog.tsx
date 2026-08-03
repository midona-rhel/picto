import { useEffect, useRef, useState } from 'react';
import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { GlassInput } from '../../../shared/ui/GlassInput/GlassInput';
import { ActionButton } from './ActionButton';
import styles from '../SubscriptionsScreen.module.css';

export interface RenameTarget {
  kind: 'subscription' | 'group';
  id: string;
  currentName: string;
}

/** Rename a subscription or group. Enter commits, Escape closes. */
export function RenameDialog({
  target,
  busy,
  onRename,
  onClose,
}: {
  target: RenameTarget | null;
  busy: boolean;
  onRename: (target: RenameTarget, name: string) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (target) {
      setName(target.currentName);
      // Focus after the modal mounts.
      requestAnimationFrame(() => {
        inputRef.current?.focus();
        inputRef.current?.select();
      });
    }
  }, [target]);

  const commit = () => {
    if (!target) return;
    const trimmed = name.trim();
    if (!trimmed || trimmed === target.currentName) {
      onClose();
      return;
    }
    onRename(target, trimmed);
  };

  return (
    <GlassModal
      open={target != null}
      onClose={onClose}
      title={target?.kind === 'group' ? 'Rename Group' : 'Rename Subscription'}
      size="sm"
      footer={
        <div className={styles.inlineActions}>
          <ActionButton variant="ghost" onClick={onClose} disabled={busy}>
            Cancel
          </ActionButton>
          <ActionButton variant="primary" onClick={commit} disabled={busy || !name.trim()}>
            Rename
          </ActionButton>
        </div>
      }
    >
      <GlassInput
        ref={inputRef}
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
        }}
        placeholder="Name"
      />
    </GlassModal>
  );
}

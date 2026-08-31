import { useEffect, useRef, useState } from 'react';
import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { GlassInput } from '../../../shared/ui/GlassInput/GlassInput';
import { ActionButton } from './ActionButton';
import styles from '../SubscriptionsScreen.module.css';
import { t } from '../../../i18n';

export interface RenameTarget {
  kind: 'subscription';
  id: string;
  currentName: string;
}

/** Rename a subscription. Enter commits, Escape closes. */
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
      title={t("Rename Subscription")}
      size="sm"
      footer={
        <div className={styles.inlineActions}>
          <ActionButton variant="ghost" onClick={onClose} disabled={busy}>
            {t("Cancel")}</ActionButton>
          <ActionButton variant="primary" onClick={commit} disabled={busy || !name.trim()}>
            {t("Rename")}</ActionButton>
        </div>
      }
    >
      <GlassInput
        ref={inputRef}
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder={t("Name")}
      />
    </GlassModal>
  );
}

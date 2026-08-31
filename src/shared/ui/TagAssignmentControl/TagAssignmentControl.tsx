import { IconPlus } from '@tabler/icons-react';
import { TagChip } from '../TagChip/TagChip';
import styles from './TagAssignmentControl.module.css';
import { t } from '../../../i18n';

function splitTag(tag: string): { namespace: string; subtag: string } {
  const separator = tag.indexOf(':');
  return separator < 0
    ? { namespace: '', subtag: tag }
    : { namespace: tag.slice(0, separator), subtag: tag.slice(separator + 1) };
}

export function TagAssignmentControl({
  tags,
  label,
  onRemove,
  onOpen,
}: {
  tags: string[];
  label?: string;
  onRemove: (tag: string) => void;
  onOpen: (button: HTMLButtonElement) => void;
}) {
  return (
    <div className={styles.field}>
      <span>{label ?? t('Automatically add tags')}</span>
      <div className={styles.values}>
        {tags.map((tag) => {
          const value = splitTag(tag);
          return (
            <TagChip
              key={tag}
              namespace={value.namespace}
              subtag={value.subtag}
              onRemove={() => onRemove(tag)}
            />
          );
        })}
        <button
          type="button"
          className={tags.length === 0 ? styles.empty : styles.add}
          onClick={(event) => onOpen(event.currentTarget)}
        >
          <IconPlus size={14} />
          {tags.length === 0 && <span>{t("Add tags")}</span>}
        </button>
      </div>
    </div>
  );
}

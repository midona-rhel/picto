import { GroupSurface } from './GroupSurface';
import styles from './GroupQuickLook.module.css';

interface GroupQuickLookContentProps {
  groupId: number;
  currentIndex: number;
  totalCount: number;
  onNavigate: (delta: number) => void;
  onClose: () => void;
}

export function GroupQuickLookContent({
  groupId,
  currentIndex,
  totalCount,
  onNavigate,
  onClose,
}: GroupQuickLookContentProps) {
  return (
    <div className={styles.frame} data-group-quick-look>
      <GroupSurface
        key={groupId}
        groupId={groupId}
        presentation="quicklook"
        rootCurrentIndex={currentIndex}
        rootTotal={totalCount}
        onNavigateRoot={onNavigate}
        onClose={onClose}
      />
    </div>
  );
}

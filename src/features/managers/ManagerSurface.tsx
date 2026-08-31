import { DuplicatesScreen } from '../duplicates/DuplicatesScreen';
import { SubscriptionsScreen } from '../subscriptions/SubscriptionsScreen';
import { TagManagerScreen } from '../tags/TagManagerScreen';
import { t } from '../../i18n';

export type ManagerNodeId = 'system:subscriptions' | 'system:duplicates' | 'system:tag_manager';

export function isManagerNodeId(nodeId: string): nodeId is ManagerNodeId {
  return nodeId === 'system:subscriptions'
    || nodeId === 'system:duplicates'
    || nodeId === 'system:tag_manager';
}

export function ManagerSurface({ nodeId }: { nodeId: string }) {
  switch (nodeId) {
    case 'system:subscriptions':
      return <SubscriptionsScreen />;
    case 'system:duplicates':
      return <DuplicatesScreen />;
    case 'system:tag_manager':
      return <TagManagerScreen />;
    default:
      return <div>{t("This view is not available.")}</div>;
  }
}

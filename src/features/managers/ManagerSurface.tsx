import { DuplicatesScreen } from '../duplicates/DuplicatesScreen';
import { SubscriptionsScreen } from '../subscriptions/SubscriptionsScreen';

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
      return <div>Tag Manager is not available yet.</div>;
    default:
      return <div>This view is not available.</div>;
  }
}

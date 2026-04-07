import { atom } from 'jotai';
import type { SubscriptionWorkspaceSnapshot } from '../shared/types/subscriptionsWorkspace';
import type {
  FailedPostGroup,
  SubscriptionIssueRecord,
  SubscriptionProgressEvent,
  SubscriptionRunRecord,
} from '../shared/types/subscriptions';
import { getProgressBySubscriptionId } from '../shared/lib/subscriptionHelpers';

export type SubscriptionCreateFormState = {
  name: string;
  initialPostLimit: string;
  periodicPostLimit: string;
};

export type SubscriptionDetailTab = 'queries' | 'failed' | 'runs';

export type SubscriptionDetailState = {
  loading: boolean;
  error: string | null;
  subscriptionId: string | null;
  runs: SubscriptionRunRecord[];
  issues: SubscriptionIssueRecord[];
  failedPosts: FailedPostGroup[];
};

export const EMPTY_SUBSCRIPTION_CREATE_FORM: SubscriptionCreateFormState = {
  name: '',
  initialPostLimit: '100',
  periodicPostLimit: '50',
};

export const EMPTY_SUBSCRIPTION_DETAIL_STATE: SubscriptionDetailState = {
  loading: false,
  error: null,
  subscriptionId: null,
  runs: [],
  issues: [],
  failedPosts: [],
};

export const subscriptionsWorkspaceSnapshotAtom = atom<SubscriptionWorkspaceSnapshot | null>(null);
export const subscriptionsWorkspaceLoadingAtom = atom(true);
export const subscriptionsWorkspaceErrorAtom = atom<string | null>(null);
export const subscriptionsSelectedSubscriptionIdAtom = atom<string | null>(null);
export const subscriptionsActiveDetailTabAtom = atom<SubscriptionDetailTab>('queries');
export const subscriptionsDetailAtom = atom<SubscriptionDetailState>(EMPTY_SUBSCRIPTION_DETAIL_STATE);
export const subscriptionsShowCreateFormAtom = atom(false);
export const subscriptionsCreateFormAtom = atom<SubscriptionCreateFormState>(EMPTY_SUBSCRIPTION_CREATE_FORM);
export const subscriptionsCreateBusyAtom = atom(false);
export const subscriptionsQuerySiteIdAtom = atom('');
export const subscriptionsQueryDraftAtom = atom('');
export const subscriptionsQueryAddBusyAtom = atom(false);
export const subscriptionsBusyKeyAtom = atom<string | null>(null);

export const subscriptionsProgressBySubscriptionIdAtom = atom<Map<string, SubscriptionProgressEvent>>((get) =>
  getProgressBySubscriptionId(get(subscriptionsWorkspaceSnapshotAtom)?.runningProgress ?? []),
);

export const subscriptionsSelectedSubscriptionAtom = atom((get) => {
  const snapshot = get(subscriptionsWorkspaceSnapshotAtom);
  const selectedId = get(subscriptionsSelectedSubscriptionIdAtom);
  return snapshot?.subscriptions.find((subscription) => subscription.id === selectedId) ?? null;
});

export const subscriptionsSelectedProgressAtom = atom((get) => {
  const selected = get(subscriptionsSelectedSubscriptionAtom);
  if (!selected) return null;
  return get(subscriptionsProgressBySubscriptionIdAtom).get(selected.id) ?? null;
});

import { atom } from 'jotai';
import type { SubscriptionWorkspaceSnapshot } from '../shared/types/subscriptionsWorkspace';
import type {
  FailedPostGroup,
  SubscriptionIssueRecord,
  SubscriptionProgressEvent,
  SubscriptionRunRecord,
} from '../shared/types/subscriptions';
import { getProgressBySubscriptionId } from '../shared/lib/subscriptionHelpers';

export type SubscriptionsSelection =
  | { kind: 'subscription'; id: string }
  | { kind: 'group'; id: string }
  | null;

export type SubscriptionDetailTab = 'queries' | 'health' | 'history';

export type SubscriptionDetailState = {
  loading: boolean;
  error: string | null;
  subscriptionId: string | null;
  runs: SubscriptionRunRecord[];
  issues: SubscriptionIssueRecord[];
  failedPosts: FailedPostGroup[];
};

export type SubscriptionsWizardState = {
  open: boolean;
  /** Pre-selected site when opened from a site-specific affordance. */
  initialSiteId: string | null;
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
export const subscriptionsSelectionAtom = atom<SubscriptionsSelection>(null);
export const subscriptionsDetailTabAtom = atom<SubscriptionDetailTab>('queries');
export const subscriptionsDetailAtom = atom<SubscriptionDetailState>(EMPTY_SUBSCRIPTION_DETAIL_STATE);
export const subscriptionsWizardAtom = atom<SubscriptionsWizardState>({ open: false, initialSiteId: null });
export const subscriptionsAccountsModalAtom = atom<{ open: boolean; focusSiteId: string | null }>({
  open: false,
  focusSiteId: null,
});
export const subscriptionsBusyKeyAtom = atom<string | null>(null);

export const subscriptionsProgressBySubscriptionIdAtom = atom<Map<string, SubscriptionProgressEvent>>((get) =>
  getProgressBySubscriptionId(get(subscriptionsWorkspaceSnapshotAtom)?.runningProgress ?? []),
);

export const subscriptionsSelectedSubscriptionAtom = atom((get) => {
  const snapshot = get(subscriptionsWorkspaceSnapshotAtom);
  const selection = get(subscriptionsSelectionAtom);
  if (!snapshot || selection?.kind !== 'subscription') return null;
  return snapshot.subscriptions.find((subscription) => subscription.id === selection.id) ?? null;
});

export const subscriptionsSelectedGroupAtom = atom((get) => {
  const snapshot = get(subscriptionsWorkspaceSnapshotAtom);
  const selection = get(subscriptionsSelectionAtom);
  if (!snapshot || selection?.kind !== 'group') return null;
  return snapshot.groups.find((group) => group.id === selection.id) ?? null;
});

export const subscriptionsSelectedProgressAtom = atom((get) => {
  const selected = get(subscriptionsSelectedSubscriptionAtom);
  if (!selected) return null;
  return get(subscriptionsProgressBySubscriptionIdAtom).get(selected.id) ?? null;
});

/** Query id currently being processed for the selected subscription. */
export const subscriptionsActiveQueryIdAtom = atom((get) => {
  const progress = get(subscriptionsSelectedProgressAtom);
  return progress?.query_id ?? null;
});

/** Credential presence by canonical site category — drives auth chips and wizard gating. */
export const subscriptionsCredentialBySiteAtom = atom((get) => {
  const snapshot = get(subscriptionsWorkspaceSnapshotAtom);
  const map = new Map<string, { type: string; health: string | null }>();
  if (!snapshot) return map;
  for (const credential of snapshot.credentials) {
    map.set(credential.site_category, { type: credential.credential_type, health: null });
  }
  for (const health of snapshot.credentialHealth) {
    const entry = map.get(health.site_category);
    if (entry) entry.health = health.health_status;
  }
  return map;
});

/** Failed posts for the selected subscription, grouped by failure reason. */
export const subscriptionsHealthGroupsAtom = atom((get) => {
  const detail = get(subscriptionsDetailAtom);
  const groups = new Map<string, FailedPostGroup[]>();
  for (const post of detail.failedPosts) {
    const reason = post.lastError?.trim() || 'Unknown failure';
    // Group by the leading portion of the error so variants collapse together.
    const key = reason.length > 80 ? `${reason.slice(0, 80)}…` : reason;
    const bucket = groups.get(key);
    if (bucket) bucket.push(post);
    else groups.set(key, [post]);
  }
  return groups;
});

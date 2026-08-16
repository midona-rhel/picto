import type {
  CredentialDomain,
  CredentialHealth,
  SubscriptionInfo,
  SubscriptionIssueRecord,
  SubscriptionProgressEvent,
  SubscriptionSiteInfo,
} from './subscriptions';

export interface AuthSiteSnapshot {
  site: SubscriptionSiteInfo;
  subscriptions: SubscriptionInfo[];
  queryCount: number;
  credential: CredentialDomain | null;
  health: CredentialHealth | null;
  issues: SubscriptionIssueRecord[];
}

export interface AuthWorkspaceSnapshot {
  sites: AuthSiteSnapshot[];
}

export interface SubscriptionListMetrics {
  failedPostCount: number;
  openIssueCount: number;
  lastActivityAt: string | null;
}

export interface SubscriptionWorkspaceSnapshot {
  subscriptions: SubscriptionInfo[];
  sites: SubscriptionSiteInfo[];
  credentials: CredentialDomain[];
  credentialHealth: CredentialHealth[];
  runningSubscriptionIds: string[];
  runningProgress: SubscriptionProgressEvent[];
  listMetrics: Record<string, SubscriptionListMetrics>;
}

import { api } from '#desktop/api';
import type {
  CredentialDomain,
  CredentialHealth,
  CredentialType,
  FlowFinishedEvent,
  FlowProgressEvent,
  SiteMetadataSchema,
  SiteMetadataValidationResult,
  SubscriptionFinishedEvent,
  SubscriptionProgressEvent,
  SubscriptionSiteInfo,
} from '../../shared/types/api';

export type {
  CredentialDomain,
  CredentialHealth,
  CredentialType,
  FlowFinishedEvent,
  FlowProgressEvent,
  SiteMetadataSchema,
  SiteMetadataValidationResult,
  SubscriptionFinishedEvent,
  SubscriptionProgressEvent,
  SubscriptionSiteInfo,
};

export interface CreatedSubscription {
  id: string;
  name: string;
  site_id: string;
  paused: boolean;
  initial_file_limit: number;
  periodic_file_limit: number;
  queries: Array<{ id: string; query_text: string; paused: boolean }>;
}

export interface CreatedSubscriptionQuery {
  id: string;
  query_text: string;
  paused: boolean;
}

export const subscriptionApi = {
  getRunningSubscriptions(): Promise<string[]> {
    return api.subscriptions.getRunning();
  },

  getRunningSubscriptionProgress(): Promise<SubscriptionProgressEvent[]> {
    return api.subscriptions.getRunningProgress();
  },

  getSiteCatalog(): Promise<SubscriptionSiteInfo[]> {
    return api.subscriptions.getSites();
  },

  listCredentials(): Promise<CredentialDomain[]> {
    return api.subscriptions.listCredentials();
  },

  listCredentialHealth(): Promise<CredentialHealth[]> {
    return api.subscriptions.listCredentialHealth();
  },

  setCredential(args: {
    siteCategory: string;
    credentialType: CredentialType;
    displayName?: string | null;
    username?: string | null;
    password?: string | null;
    cookies?: Record<string, string> | null;
    oauthToken?: string | null;
  }): Promise<void> {
    return api.subscriptions.setCredential({
      site_category: args.siteCategory,
      credential_type: args.credentialType,
      display_name: args.displayName,
      username: args.username,
      password: args.password,
      cookies: args.cookies,
      oauth_token: args.oauthToken,
    });
  },

  deleteCredential(siteCategory: string): Promise<void> {
    return api.subscriptions.deleteCredential(siteCategory);
  },

  createSubscription(args: {
    name: string;
    siteId: string;
    queries: string[];
    flowId?: number | null;
    initialFileLimit?: number | null;
    periodicFileLimit?: number | null;
  }): Promise<CreatedSubscription> {
    return api.subscriptions.create({
      name: args.name,
      site_id: args.siteId,
      queries: args.queries,
      flow_id: args.flowId ?? undefined,
      initial_file_limit: args.initialFileLimit ?? undefined,
      periodic_file_limit: args.periodicFileLimit ?? undefined,
    }) as Promise<CreatedSubscription>;
  },

  resetSubscription(args: { id: string }): Promise<void> {
    return api.subscriptions.reset(args.id);
  },

  runSubscriptionQuery(args: { queryId: string; subscriptionId: string }): Promise<void> {
    return api.subscriptions.runQuery(args.subscriptionId, args.queryId) as Promise<void>;
  },

  deleteSubscriptionQuery(args: { id: string }): Promise<void> {
    return api.subscriptions.deleteQuery(args.id);
  },

  addSubscriptionQuery(args: { subscriptionId: string; queryText: string }): Promise<CreatedSubscriptionQuery> {
    return api.subscriptions.addQuery(args.subscriptionId, args.queryText) as Promise<CreatedSubscriptionQuery>;
  },

  getSubscriptionGroups<T>(): Promise<T[]> {
    return api.flows.list() as Promise<T[]>;
  },

  createSubscriptionGroup(args: { name: string; schedule?: string }): Promise<unknown> {
    return api.flows.create(args.name, args.schedule);
  },

  deleteSubscriptionGroup(args: { id: string; deleteFiles?: boolean }): Promise<void> {
    return api.flows.delete(args.id, args.deleteFiles);
  },

  renameSubscriptionGroup(args: { id: string; name: string }): Promise<void> {
    return api.flows.rename(args.id, args.name);
  },

  setSubscriptionGroupSchedule(args: { id: string; schedule: string }): Promise<void> {
    return api.flows.setSchedule(args.id, args.schedule);
  },

  runSubscriptionGroup(args: { id: string }): Promise<void> {
    return api.flows.run(args.id);
  },

  stopSubscriptionGroup(args: { id: string }): Promise<void> {
    return api.flows.stop(args.id);
  },
};

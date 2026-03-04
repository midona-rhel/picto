import { api, listen, type UnlistenFn } from '#desktop/api';

// Re-export event types from central api types for backwards compatibility.
export type {
  SubscriptionProgressEvent,
  SubscriptionStartedEvent,
  SubscriptionFinishedEvent,
  FlowStartedEvent,
  FlowProgressEvent,
  FlowFinishedEvent,
  SubscriptionSiteInfo,
  SiteMetadataSchema,
  SiteMetadataValidationResult,
  CredentialDomain,
  CredentialHealth,
  CredentialType,
} from '../types/api';
import type {
  SubscriptionProgressEvent,
  SubscriptionStartedEvent,
  SubscriptionFinishedEvent,
  FlowStartedEvent,
  FlowProgressEvent,
  FlowFinishedEvent,
  SubscriptionSiteInfo,
  SiteMetadataSchema,
  SiteMetadataValidationResult,
  CredentialDomain,
  CredentialHealth,
  CredentialType,
} from '../types/api';

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

/**
 * SubscriptionController — frontend facade for subscription command/event
 * orchestration. Keeps component code from owning direct transport calls.
 */
export const SubscriptionController = {
  getRunningSubscriptions(): Promise<string[]> {
    return api.subscriptions.getRunning();
  },

  getRunningSubscriptionProgress(): Promise<SubscriptionProgressEvent[]> {
    return api.subscriptions.getRunningProgress();
  },

  getSubscriptions<T>(): Promise<T[]> {
    return api.subscriptions.list() as Promise<T[]>;
  },

  getSites<T>(): Promise<T[]> {
    return api.subscriptions.getSites() as Promise<T[]>;
  },

  getSiteCatalog(): Promise<SubscriptionSiteInfo[]> {
    return api.subscriptions.getSites();
  },

  getSiteMetadataSchema(siteId: string): Promise<SiteMetadataSchema> {
    return api.subscriptions.getSiteMetadataSchema(siteId);
  },

  validateSiteMetadata(args: {
    siteId: string;
    sampleUrl?: string;
    sampleMetadataJson?: Record<string, unknown> | null;
  }): Promise<SiteMetadataValidationResult> {
    return api.subscriptions.validateSiteMetadata({
      site_id: args.siteId,
      sample_url: args.sampleUrl,
      sample_metadata_json: args.sampleMetadataJson,
    });
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

  pauseSubscription(args: { id: string; paused: boolean }): Promise<void> {
    return api.subscriptions.pause(args.id, args.paused);
  },

  deleteSubscription(args: { id: string; deleteFiles: boolean }): Promise<number> {
    return api.subscriptions.delete(args.id, args.deleteFiles);
  },

  runSubscription(args: { id: string }): Promise<void> {
    return api.subscriptions.run(args.id);
  },

  stopSubscription(args: { id: string }): Promise<void> {
    return api.subscriptions.stop(args.id);
  },

  resetSubscription(args: { id: string }): Promise<void> {
    return api.subscriptions.reset(args.id);
  },

  runSubscriptionQuery(args: { queryId: string; subscriptionId: string }): Promise<void> {
    return api.subscriptions.runQuery(args.subscriptionId, args.queryId) as Promise<void>;
  },

  pauseSubscriptionQuery(args: { id: string; paused: boolean }): Promise<void> {
    return api.subscriptions.pauseQuery(args.id, args.paused);
  },

  deleteSubscriptionQuery(args: { id: string }): Promise<void> {
    return api.subscriptions.deleteQuery(args.id);
  },

  addSubscriptionQuery(args: { subscriptionId: string; queryText: string }): Promise<CreatedSubscriptionQuery> {
    return api.subscriptions.addQuery(args.subscriptionId, args.queryText) as Promise<CreatedSubscriptionQuery>;
  },

  renameSubscription(args: { id: string; name: string }): Promise<void> {
    return api.subscriptions.rename(args.id, args.name);
  },

  getFlows<T>(): Promise<T[]> {
    return api.flows.list() as Promise<T[]>;
  },

  createFlow(args: { name: string; schedule?: string }): Promise<unknown> {
    return api.flows.create(args.name, args.schedule);
  },

  deleteFlow(args: { id: string; deleteFiles?: boolean }): Promise<void> {
    return api.flows.delete(args.id, args.deleteFiles);
  },

  renameFlow(args: { id: string; name: string }): Promise<void> {
    return api.flows.rename(args.id, args.name);
  },

  setFlowSchedule(args: { id: string; schedule: string }): Promise<void> {
    return api.flows.setSchedule(args.id, args.schedule);
  },

  runFlow(args: { id: string }): Promise<void> {
    return api.flows.run(args.id);
  },

  stopFlow(args: { id: string }): Promise<void> {
    return api.flows.stop(args.id);
  },

  // PBI-042: Typed lifecycle event listeners.
  onStarted(handler: (event: SubscriptionStartedEvent) => void): Promise<UnlistenFn> {
    return listen<SubscriptionStartedEvent>('subscription-started', (event) => handler(event.payload));
  },

  onProgress(handler: (event: SubscriptionProgressEvent) => void): Promise<UnlistenFn> {
    return listen<SubscriptionProgressEvent>('subscription-progress', (event) =>
      handler(event.payload)
    );
  },

  onFinished(handler: (event: SubscriptionFinishedEvent) => void): Promise<UnlistenFn> {
    return listen<SubscriptionFinishedEvent>('subscription-finished', (event) => handler(event.payload));
  },

  // PBI-047: Flow lifecycle event listeners.
  onFlowStarted(handler: (event: FlowStartedEvent) => void): Promise<UnlistenFn> {
    return listen<FlowStartedEvent>('flow-started', (event) => handler(event.payload));
  },

  onFlowProgress(handler: (event: FlowProgressEvent) => void): Promise<UnlistenFn> {
    return listen<FlowProgressEvent>('flow-progress', (event) => handler(event.payload));
  },

  onFlowFinished(handler: (event: FlowFinishedEvent) => void): Promise<UnlistenFn> {
    return listen<FlowFinishedEvent>('flow-finished', (event) => handler(event.payload));
  },
};

import type {
  SubscriptionProgressEvent,
  SubscriptionSiteInfo,
} from '../types/subscriptions';

export function getCredentialOwnerSiteId(
  siteId: string,
  sites: SubscriptionSiteInfo[],
): string {
  return sites.find((site) => site.id === siteId)?.credential_owner_site_id ?? siteId;
}

export function getProgressBySubscriptionId(progress: SubscriptionProgressEvent[]): Map<string, SubscriptionProgressEvent> {
  return new Map(progress.map((entry) => [entry.subscription_id, entry]));
}

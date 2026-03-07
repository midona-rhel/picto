import type { SubscriptionFinishedEvent } from '../../../shared/types/api/core';
import type { FlowInfo, SitePluginInfo, SubProgress } from '../types';

export function canonicalSiteId(siteId: string): string {
  const normalized = siteId.trim().toLowerCase();
  switch (normalized) {
    case 'rule34xxx':
    case 'rule34.xxx':
      return 'rule34';
    case 'e621.net':
      return 'e621';
    case 'furaffinity.net':
      return 'furaffinity';
    case 'yande.re':
      return 'yandere';
    case 'kemono.party':
      return 'kemono';
    case 'coomer.party':
      return 'coomer';
    case 'baraag.net':
      return 'baraag';
    case 'pawoo.net':
      return 'pawoo';
    default:
      return normalized;
  }
}

export function hasCredentialForSite(siteId: string, credentialSites: Set<string>): boolean {
  const canonical = canonicalSiteId(siteId);
  return credentialSites.has(canonical) || credentialSites.has(siteId.trim().toLowerCase());
}

export function formatRelativeTime(iso: string | null | undefined): string {
  if (!iso) return 'Never';
  const date = new Date(iso);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 1) return 'Just now';
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHrs = Math.floor(diffMin / 60);
  if (diffHrs < 24) return `${diffHrs}h ago`;
  const diffDays = Math.floor(diffHrs / 24);
  return `${diffDays}d ago`;
}

export function flattenQueries(flow: FlowInfo, sites: SitePluginInfo[], credentialSites: Set<string>) {
  const result: {
    queryId: string;
    queryText: string;
    siteName: string;
    sitePluginId: string;
    backendSubId: string;
    filesFound: number;
    lastCheck: string | null;
    paused: boolean;
    missingAuth: boolean;
  }[] = [];
  for (const sub of flow.subscriptions) {
    const siteIdRaw = sub.site_id ?? sub.site_plugin_id ?? '';
    const siteId = canonicalSiteId(siteIdRaw);
    const site = sites.find((s) => canonicalSiteId(s.id) === siteId);
    const siteName = site?.name ?? siteIdRaw;
    const missingAuth = Boolean(
      site?.auth_supported &&
      site?.auth_required_for_full_access &&
      !hasCredentialForSite(siteId, credentialSites),
    );
    for (const q of sub.queries) {
      const label = (q.display_name ?? '').trim() || q.query_text.trim() || `Query ${q.id}`;
      result.push({
        queryId: q.id,
        queryText: label,
        siteName,
        sitePluginId: siteId,
        backendSubId: sub.id,
        filesFound: q.files_found,
        lastCheck: q.last_check_time,
        paused: q.paused,
        missingAuth,
      });
    }
  }
  return result;
}

export function getLastRan(flow: FlowInfo): string | null {
  let latest: string | null = null;
  for (const sub of flow.subscriptions) {
    for (const q of sub.queries) {
      if (!q.last_check_time) continue;
      if (!latest || q.last_check_time > latest) latest = q.last_check_time;
    }
  }
  return latest;
}

export function getFlowProgress(flow: FlowInfo, progressMap: Map<string, SubProgress>): SubProgress | null {
  let total: SubProgress | null = null;
  for (const sub of flow.subscriptions) {
    const p = progressMap.get(sub.id);
    if (!p) continue;
    if (!total) {
      total = { ...p };
    } else {
      total.filesDownloaded += p.filesDownloaded;
      total.filesSkipped += p.filesSkipped;
      total.pagesFetched += p.pagesFetched;
      total.statusText = p.statusText;
    }
  }
  return total;
}

export function formatSubscriptionFailureMessage(event: SubscriptionFinishedEvent): string {
  const fallback = event.error || `${event.errors_count} error(s)`;
  if (event.failure_kind === 'unauthorized') {
    return `${fallback}. Authentication was rejected for this site.`;
  }
  if (event.failure_kind === 'expired') {
    return `${fallback}. Stored credentials appear expired.`;
  }
  if (event.failure_kind === 'rate_limited') {
    return `${fallback}. Source is currently rate-limited.`;
  }
  if (event.failure_kind === 'network') {
    return `${fallback}. Network/connectivity issue detected.`;
  }
  return fallback;
}

import { useEffect, useState } from 'react';
import { IconWorld } from '@tabler/icons-react';

const iconRequests = new Map<string, Promise<string | null>>();
const RETRY_DELAY_MS = 30_000;

function requestIcon(domain: string): Promise<string | null> {
  const existing = iconRequests.get(domain);
  if (existing) return existing;
  const request = ((window as Window & {
    picto?: { siteIcons?: { get?: (value: string) => Promise<string | null> } };
  }).picto?.siteIcons?.get?.(domain) ?? Promise.resolve(null)).then((source) => {
    if (!source) iconRequests.delete(domain);
    return source;
  }, () => {
    iconRequests.delete(domain);
    return null;
  });
  iconRequests.set(domain, request);
  return request;
}

export function SiteIcon({ domain, size }: { domain: string; size: number }) {
  const [source, setSource] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    let retryTimer: number | undefined;
    setSource(null);
    setFailed(false);

    const load = () => {
      void requestIcon(domain).then((next) => {
        if (!active) return;
        setSource(next);
        if (!next) retryTimer = window.setTimeout(load, RETRY_DELAY_MS);
      });
    };
    load();

    return () => {
      active = false;
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
    };
  }, [domain]);

  if (!source || failed) return <IconWorld size={size} stroke={1.5} />;
  return <img src={source} alt="" width={size} height={size} onError={() => setFailed(true)} />;
}

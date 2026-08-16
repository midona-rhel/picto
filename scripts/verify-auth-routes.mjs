import { getStaticAuthLoginRoutes } from '../electron/windows/authSessions.mjs';

const ERROR_PAGE_PATTERN = /record was not found|page not found|not found \(404\)|connect error/i;
const MANUAL_STATUS_CODES = new Set([401, 403, 429]);

export async function verifyAuthRoute({ site, loginUrl }, fetchImpl = fetch) {
  const response = await fetchImpl(loginUrl, {
    redirect: 'follow',
    headers: { 'user-agent': 'Mozilla/5.0 Picto auth-route verifier' },
    signal: AbortSignal.timeout(30_000),
  });
  const body = await response.text();
  const title = body.match(/<title[^>]*>([^<]*)<\/title>/i)?.[1]?.trim() ?? '';

  if (MANUAL_STATUS_CODES.has(response.status)) {
    return { site, status: 'manual', detail: `HTTP ${response.status} ${response.url}` };
  }
  if (
    !response.ok
    || body.trim().length === 0
    || ERROR_PAGE_PATTERN.test(title)
    || ERROR_PAGE_PATTERN.test(body.slice(0, 20_000))
  ) {
    return {
      site,
      status: 'failed',
      detail: `HTTP ${response.status} ${response.url}${title ? ` (${title})` : ''}`,
    };
  }
  return { site, status: 'passed', detail: `HTTP ${response.status} ${response.url}${title ? ` (${title})` : ''}` };
}

export async function verifyAuthRoutes(routes = getStaticAuthLoginRoutes()) {
  return Promise.all(
    routes.map(async (route) => {
      try {
        return await verifyAuthRoute(route);
      } catch (error) {
        return { site: route.site, status: 'failed', detail: error instanceof Error ? error.message : String(error) };
      }
    }),
  );
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const results = await verifyAuthRoutes();

  for (const result of results) {
    const marker = result.status === 'passed' ? 'PASS' : result.status === 'manual' ? 'MANUAL' : 'FAIL';
    console.log(`${marker.padEnd(6)} ${result.site.padEnd(14)} ${result.detail}`);
  }

  const failed = results.filter((result) => result.status === 'failed');
  if (failed.length > 0) {
    process.exitCode = 1;
  }
}

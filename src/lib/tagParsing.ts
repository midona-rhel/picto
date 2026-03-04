const INGEST_ALLOWED_NAMESPACES = new Set<string>([
  'creator',
  'studio',
  'character',
  'person',
  'series',
  'species',
  'meta',
  'system',
  'artist',
  'copyright',
  'general',
  'rating',
  'source',
]);

function isValidNamespaceCandidate(value: string): boolean {
  if (!value) return true;
  const first = value[0];
  if (!/[A-Za-z]/.test(first)) return false;
  for (let i = 1; i < value.length; i += 1) {
    const ch = value[i];
    if (!/[A-Za-z0-9 _-]/.test(ch)) return false;
  }
  return true;
}

/**
 * Parse a tag display/raw string with backend-compatible namespace semantics.
 * Unknown namespace prefixes are treated as literal tag text.
 */
export function parseTagString(rawTag: string): { namespace: string; subtag: string } {
  const idx = rawTag.indexOf(':');
  if (idx <= 0) return { namespace: '', subtag: rawTag };

  const candidate = rawTag.slice(0, idx);
  if (!isValidNamespaceCandidate(candidate)) {
    return { namespace: '', subtag: rawTag };
  }

  const lowered = candidate.toLowerCase();
  if (!INGEST_ALLOWED_NAMESPACES.has(lowered)) {
    return { namespace: '', subtag: rawTag };
  }

  return {
    namespace: candidate,
    subtag: rawTag.slice(idx + 1),
  };
}

export function extractNamespace(rawTag: string): string {
  return parseTagString(rawTag).namespace;
}

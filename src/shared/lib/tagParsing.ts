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
 * Parse a tag display/raw string with backend `parse_tag`-compatible semantics.
 *
 * This is intentionally generic parser behavior, not ingest policy. The backend
 * may coerce unknown namespaces to literals on external ingest paths, but the
 * renderer should not re-apply that rule to already-stored or user-entered tags.
 */
export function parseTagString(rawTag: string): { namespace: string; subtag: string } {
  const idx = rawTag.indexOf(':');
  if (idx <= 0) return { namespace: '', subtag: rawTag };

  const candidate = rawTag.slice(0, idx);
  if (!isValidNamespaceCandidate(candidate)) {
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

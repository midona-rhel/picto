interface HashItem {
  entity_hash: string;
}

export function hasSameEntityOrder(previous: readonly string[], next: readonly HashItem[]): boolean {
  return previous.length === next.length
    && previous.every((hash, index) => hash === next[index]?.entity_hash);
}

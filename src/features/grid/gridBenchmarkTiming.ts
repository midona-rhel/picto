// Opt-in instrumentation in optimized benchmark packages. Ordinary packages
// compile these branches out; no navigation, media or readiness behavior changes.
const enabled = import.meta.env?.VITE_BROWSE_BENCHMARK === '1';
const prepared = new WeakMap<object, number>();
const commits = new WeakMap<object, { started: number; preparation: number; count: number }>();

export function beginGridPreparation(): number {
  return enabled ? performance.now() : 0;
}
export function endGridPreparation(items: object, started: number): void {
  if (enabled) prepared.set(items, performance.now() - started);
}
export function beginGridCommit(items: readonly unknown[]): void {
  if (enabled) commits.set(items, {
    started: performance.now(), preparation: prepared.get(items) ?? 0, count: items.length,
  });
}
export function finishGridCommit(items: object): void {
  if (!enabled) return;
  const commit = commits.get(items);
  if (!commit) return;
  commits.delete(items);
  prepared.delete(items);
  const end = performance.now();
  // The interval includes scheduling until the first valid base draw. It is a
  // conservative upper bound on preparation/commit CPU, not GPU presentation.
  performance.measure('picto:grid-preparation-commit', {
    start: commit.started - commit.preparation, end,
    detail: { items: commit.count, preparation_ms: commit.preparation,
      commit_to_base_ms: end - commit.started },
  });
}

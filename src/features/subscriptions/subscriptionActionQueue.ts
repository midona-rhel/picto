export function createSubscriptionActionQueue() {
  let tail: Promise<unknown> = Promise.resolve();

  return function enqueue<T>(action: () => Promise<T>): Promise<T> {
    const pending = tail.then(action, action);
    tail = pending.then(() => undefined, () => undefined);
    return pending;
  };
}

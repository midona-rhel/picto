import type { UnlistenFn } from './ipc';

export interface Store {
  get<T = unknown>(key: string): Promise<T | null>;
  set(key: string, value: unknown): Promise<void>;
  save(): Promise<void>;
  onKeyChange(key: string, handler: (value: unknown) => void): Promise<UnlistenFn>;
}

const channels = new Map<string, BroadcastChannel>();

function getChannel(name: string): BroadcastChannel {
  let channel = channels.get(name);
  if (!channel) {
    channel = new BroadcastChannel(name);
    channels.set(name, channel);
  }
  return channel;
}

class LocalStore implements Store {
  private namespace: string;
  private state: Record<string, unknown>;

  constructor(name: string) {
    this.namespace = `picto:store:${name}`;
    const raw = localStorage.getItem(this.namespace);
    // PBI-036: Harden deserialization — quarantine corrupt data, reset to empty.
    if (raw) {
      try {
        const parsed = JSON.parse(raw);
        this.state = (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) ? parsed : {};
      } catch {
        console.warn(`[LocalStore] corrupt JSON in "${this.namespace}"; quarantining and resetting`);
        try { localStorage.setItem(`${this.namespace}:quarantine`, raw); } catch { /* best effort */ }
        this.state = {};
      }
    } else {
      this.state = {};
    }
  }

  async get<T = unknown>(key: string): Promise<T | null> {
    return (this.state[key] as T | undefined) ?? null;
  }

  async set(key: string, value: unknown): Promise<void> {
    this.state[key] = value;
  }

  async save(): Promise<void> {
    localStorage.setItem(this.namespace, JSON.stringify(this.state));
    getChannel(this.namespace).postMessage({ type: 'save', payload: this.state });
  }

  async onKeyChange(key: string, handler: (value: unknown) => void): Promise<UnlistenFn> {
    const channel = getChannel(this.namespace);
    const listener = (event: MessageEvent) => {
      if (event.data?.type !== 'save') return;
      const next = event.data.payload ?? {};
      if (Object.prototype.hasOwnProperty.call(next, key)) handler(next[key]);
    };
    channel.addEventListener('message', listener);
    return () => channel.removeEventListener('message', listener);
  }
}

export async function load(name: string, _options?: { autoSave?: boolean }): Promise<Store> {
  return new LocalStore(name);
}

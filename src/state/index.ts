/**
 * Frontend state ownership layer (Jotai atoms).
 *
 * Authoritative state is plain atoms.
 * Derived state is read-only atoms that compute from authoritative atoms.
 * Actions are write-only atoms that update authoritative atoms.
 *
 * Migrated slices own their state here. Legacy stores under
 * state-legacy/ remain for slices not yet cut over.
 */

export * from './sidebar';
export * from './navigation';
export * from './selection';
export * from './filters';
export * from './grid';
export * from './settings';
export * from './runtime';

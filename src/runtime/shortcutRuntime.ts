import { isEditableTarget } from '../app/editableTarget';

export type ShortcutScopeHandler = (event: KeyboardEvent) => boolean | void;

export interface ShortcutScopeOptions {
  priority?: number;
  allowInEditable?: boolean;
}

interface ShortcutScope extends Required<ShortcutScopeOptions> {
  token: symbol;
  order: number;
  handler: ShortcutScopeHandler;
}

const scopes = new Map<symbol, ShortcutScope>();
const suspensionLeases = new Set<symbol>();
let registrationOrder = 0;
let listening = false;

function orderedScopes(): ShortcutScope[] {
  return [...scopes.values()].sort((left, right) =>
    right.priority - left.priority || right.order - left.order,
  );
}

function standardTextCommand(event: KeyboardEvent): boolean {
  if (!(event.metaKey || event.ctrlKey) || event.altKey) return false;
  return ['a', 'c', 'v', 'x'].includes(event.key.toLowerCase());
}

function textSurfaceOwns(event: KeyboardEvent): boolean {
  if (isEditableTarget(event.target)) return true;
  if (event.target instanceof HTMLElement
    && event.target.closest('[data-picto-text-shortcuts]')) return true;
  const selection = window.getSelection();
  return selection != null && !selection.isCollapsed && selection.rangeCount > 0;
}

function dispatchShortcut(event: KeyboardEvent): void {
  if (suspensionLeases.size > 0) return;
  if (standardTextCommand(event) && textSurfaceOwns(event)) return;
  const editable = isEditableTarget(event.target);
  for (const scope of orderedScopes()) {
    if (editable && !scope.allowInEditable) continue;
    const handled = scope.handler(event) === true || event.defaultPrevented;
    if (!handled) continue;
    if (!event.defaultPrevented) event.preventDefault();
    event.stopPropagation();
    return;
  }
}

function syncListener(): void {
  if (scopes.size > 0 && !listening) {
    window.addEventListener('keydown', dispatchShortcut, true);
    listening = true;
  } else if (scopes.size === 0 && listening) {
    window.removeEventListener('keydown', dispatchShortcut, true);
    listening = false;
  }
}

export function registerShortcutScope(
  handler: ShortcutScopeHandler,
  options: ShortcutScopeOptions = {},
): () => void {
  const token = Symbol('shortcut-scope');
  scopes.set(token, {
    token,
    handler,
    priority: options.priority ?? 0,
    allowInEditable: options.allowInEditable ?? false,
    order: ++registrationOrder,
  });
  syncListener();
  let active = true;
  return () => {
    if (!active) return;
    active = false;
    scopes.delete(token);
    syncListener();
  };
}

/**
 * Suspends every Picto application shortcut without intercepting the event.
 * Leases are reference counted, so nested interactive runtimes restore
 * shortcuts only after the final owner releases its own lease.
 */
export function acquireShortcutSuspension(): () => void {
  const lease = Symbol('shortcut-suspension');
  suspensionLeases.add(lease);
  let active = true;
  return () => {
    if (!active) return;
    active = false;
    suspensionLeases.delete(lease);
  };
}

export function areShortcutsSuspended(): boolean {
  return suspensionLeases.size > 0;
}

export function resetShortcutRuntimeForTests(): void {
  scopes.clear();
  suspensionLeases.clear();
  registrationOrder = 0;
  syncListener();
}

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  acquireShortcutSuspension,
  areShortcutsSuspended,
  registerShortcutScope,
  resetShortcutRuntimeForTests,
} from './shortcutRuntime';

function press(key: string, target: EventTarget = window): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event;
}

describe('shortcutRuntime', () => {
  beforeEach(resetShortcutRuntimeForTests);
  afterEach(resetShortcutRuntimeForTests);

  it('dispatches deterministically by priority and stops after the handled scope', () => {
    const lower = vi.fn(() => true);
    const higher = vi.fn(() => true);
    registerShortcutScope(lower, { priority: 1 });
    registerShortcutScope(higher, { priority: 10 });

    const event = press('x');
    expect(higher).toHaveBeenCalledOnce();
    expect(lower).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(true);
  });

  it('keeps ordinary application scopes out of editable controls', () => {
    const ordinary = vi.fn(() => true);
    const modal = vi.fn(() => true);
    registerShortcutScope(ordinary);
    registerShortcutScope(modal, { priority: 100, allowInEditable: true });
    const input = document.createElement('input');
    document.body.append(input);

    press('Escape', input);
    expect(modal).toHaveBeenCalledOnce();
    expect(ordinary).not.toHaveBeenCalled();
    input.remove();
  });

  it('requires every nested suspension owner to release its own lease', () => {
    const handler = vi.fn(() => true);
    registerShortcutScope(handler);
    const releaseFirst = acquireShortcutSuspension();
    const releaseSecond = acquireShortcutSuspension();

    press('ArrowLeft');
    releaseFirst();
    press('ArrowLeft');
    expect(handler).not.toHaveBeenCalled();
    expect(areShortcutsSuspended()).toBe(true);

    releaseSecond();
    press('ArrowLeft');
    expect(handler).toHaveBeenCalledOnce();
    expect(areShortcutsSuspended()).toBe(false);
  });

  it('does not cancel keyboard events while shortcuts are suspended', () => {
    registerShortcutScope(() => true);
    const release = acquireShortcutSuspension();
    const event = press('Space');
    expect(event.defaultPrevented).toBe(false);
    release();
  });
});

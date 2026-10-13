import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import { usePersistedSet } from './usePersistedSet';

describe('persisted sidebar expansion', () => {
  beforeEach(() => localStorage.clear());

  it('restores all collapsed nodes after remounting, including batched toggles', () => {
    const key = 'picto-sidebar-collapsed';
    const nodes = ['folder:1', 'folder:2', 'smart:1'];
    const first = renderHook(() => usePersistedSet(key));
    act(() => nodes.forEach((id) => first.result.current[1](id)));
    first.unmount();

    const reopened = renderHook(() => usePersistedSet(key));
    expect([...reopened.result.current[0]]).toEqual(nodes);
    act(() => reopened.result.current[1]('folder:1'));
    reopened.unmount();

    const again = renderHook(() => usePersistedSet(key));
    expect([...again.result.current[0]]).toEqual(['folder:2', 'smart:1']);
  });
});

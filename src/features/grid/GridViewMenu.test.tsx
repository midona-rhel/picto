import { act, fireEvent, render, screen } from '@testing-library/react';
import { createStore, Provider } from 'jotai';
import { describe, expect, it, vi } from 'vitest';
import { gridSessionAtom } from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import { buildContextMenuViewEntries, buildViewMenuEntries } from './GridViewMenu';

vi.mock('../../controllers/gridController', () => ({
  gridController: { setSort: vi.fn(), updateView: vi.fn(), saveViewPref: vi.fn() },
}));

describe.each([
  ['toolbar', buildViewMenuEntries],
  ['context menu', buildContextMenuViewEntries],
] as const)('%s sorting controls', (_label, buildEntries) => {
  it('hides direction buttons for Random and restores them for an ordered sort', () => {
    const store = createStore();
    store.set(gridSessionAtom, (s) => ({ ...s, sort: { field: 'random', direction: 'descending', randomSeed: 'fixed' } }));
    const entry = buildEntries()[0];
    if (!('custom' in entry)) throw new Error('Missing view panel');
    render(<Provider store={store}>{entry.render()}</Provider>);
    expect(screen.queryByRole('button', { name: 'Ascending' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Descending' })).not.toBeInTheDocument();
    expect(screen.getByText('Sort by')).toBeInTheDocument();
    expect(store.get(gridSessionAtom).sort.randomSeed).toBe('fixed');

    act(() => store.set(gridSessionAtom, (s) => ({ ...s, sort: { field: 'name', direction: 'descending' } })));
    expect(screen.getByRole('button', { name: 'Descending' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Ascending' }));
    expect(gridController.setSort).toHaveBeenLastCalledWith('name', 'ascending');
  });
});

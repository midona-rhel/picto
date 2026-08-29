import { act, render, screen } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { describe, expect, it } from 'vitest';
import type { MenuCustom } from '../../shared/ui/ContextMenu/ContextMenu';
import { gridSessionAtom } from '../../state/grid';
import { buildViewMenuEntries } from './GridViewMenu';

const store = getDefaultStore();

function displayPanel(): MenuCustom {
  const display = buildViewMenuEntries().find(
    (entry): entry is MenuCustom => 'custom' in entry && entry.key === 'display-toggles',
  );
  if (!display) throw new Error('Display panel is missing');
  return display;
}

describe('GridViewMenu', () => {
  it('keeps transient grayscale out of persistent display preferences', () => {
    render(displayPanel().render());

    expect(screen.queryByText('Grayscale Preview')).not.toBeInTheDocument();
    expect(screen.getByText('Compact')).toBeInTheDocument();
  });

  it('offers descendant media only in ordinary folder views', () => {
    const initial = store.get(gridSessionAtom);
    act(() => store.set(gridSessionAtom, { ...initial, scope: { kind: 'folder', folder_id: 7 } }));
    const folder = render(displayPanel().render());
    expect(screen.getByText('Show Subfolder Content')).toBeInTheDocument();
    folder.unmount();

    act(() => store.set(gridSessionAtom, { ...initial, scope: { kind: 'smart_folder', smart_folder_id: 9 } }));
    render(displayPanel().render());
    expect(screen.queryByText('Show Subfolder Content')).not.toBeInTheDocument();
    act(() => store.set(gridSessionAtom, initial));
  });
});

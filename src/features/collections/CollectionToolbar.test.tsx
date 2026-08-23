import { fireEvent, render, screen } from '@testing-library/react';
import { Provider, createStore } from 'jotai';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it, vi } from 'vitest';
import { collectionChromeAtom } from '../../state/collections';
import { CollectionToolbar } from './CollectionToolbar';

describe('CollectionToolbar', () => {
  it('uses the titlebar breadcrumb and icon-only edit action', () => {
    const close = vi.fn();
    const edit = vi.fn();
    const finishEditing = vi.fn();
    const store = createStore();
    store.set(collectionChromeAtom, {
      label: 'Morning set',
      parentLabel: 'All',
      mode: 'reader',
      memberViewerOpen: false,
      close,
      edit,
      finishEditing,
    });

    render(
      <MantineProvider>
        <Provider store={store}><CollectionToolbar /></Provider>
      </MantineProvider>,
    );
    expect(screen.getByText('All')).toBeInTheDocument();
    expect(screen.getByText('Morning set')).toBeInTheDocument();
    expect(screen.queryByText('Edit Collection')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Edit collection' }));
    expect(edit).toHaveBeenCalledOnce();
  });
});

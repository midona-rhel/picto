import { fireEvent, render, screen } from '@testing-library/react';
import { Provider, createStore } from 'jotai';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it, vi } from 'vitest';
import { collectionChromeAtom } from '../../state/collections';
import { CollectionToolbar } from './CollectionToolbar';

describe('CollectionToolbar', () => {
  it('uses the standard icon action to enter collection editing', () => {
    const close = vi.fn();
    const showReader = vi.fn();
    const edit = vi.fn();
    const store = createStore();
    store.set(collectionChromeAtom, {
      label: 'Morning set',
      parentLabel: 'All',
      mode: 'reader',
      memberViewerOpen: false,
      close,
      showReader,
      edit,
    });

    render(
      <MantineProvider>
        <Provider store={store}><CollectionToolbar /></Provider>
      </MantineProvider>,
    );
    expect(screen.queryByText('Edit Collection')).not.toBeInTheDocument();
    const toolbar = screen.getByLabelText('Collection controls');
    const editButton = screen.getByRole('button', { name: 'Edit collection' });
    expect(toolbar.lastElementChild).toContainElement(editButton);
    fireEvent.click(editButton);
    expect(edit).toHaveBeenCalledOnce();
  });

  it('leaves editing through normal back navigation without a finish action', () => {
    const close = vi.fn();
    const showReader = vi.fn();
    const store = createStore();
    store.set(collectionChromeAtom, {
      label: 'Morning set',
      parentLabel: 'All',
      mode: 'editor',
      memberViewerOpen: false,
      close,
      showReader,
      edit: vi.fn(),
    });

    render(
      <MantineProvider>
        <Provider store={store}><CollectionToolbar /></Provider>
      </MantineProvider>,
    );
    expect(screen.queryByRole('button', { name: /finish/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Edit collection' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Back to collection' }));
    expect(showReader).toHaveBeenCalledOnce();
    expect(close).not.toHaveBeenCalled();
  });
});

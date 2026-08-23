import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CollectionOrganizerModal } from './CollectionOrganizerModal';

const organizeIntoCollection = vi.hoisted(() => vi.fn());
vi.mock('../../platform/entityApi', () => ({ organizeIntoCollection }));

const target = { kind: 'explicit' as const, item_ids: [1, 2] };

beforeEach(() => {
  organizeIntoCollection.mockReset();
  organizeIntoCollection.mockResolvedValue({ collection_id: 9, receipt: {} });
});

describe('CollectionOrganizerModal', () => {
  it('requires a name when creating a collection from standalone items', async () => {
    const onComplete = vi.fn();
    const onClose = vi.fn();
    render(
      <CollectionOrganizerModal
        open
        target={target}
        collections={[]}
        onClose={onClose}
        onComplete={onComplete}
      />,
    );

    fireEvent.change(await screen.findByPlaceholderText('Collection name'), {
      target: { value: 'Chapter one' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Create Collection' }));

    await waitFor(() => expect(organizeIntoCollection).toHaveBeenCalledWith({
      target,
      label: 'Chapter one',
      winning_collection_id: null,
    }));
    expect(onComplete).toHaveBeenCalledWith(9);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('requires the user to choose the winning collection when several are selected', async () => {
    render(
      <CollectionOrganizerModal
        open
        target={target}
        collections={[
          { collection_id: 7, label: 'Left', member_count: 2 },
          { collection_id: 8, label: 'Right', member_count: 4 },
        ]}
        onClose={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByText('Right'));
    fireEvent.click(screen.getByRole('button', { name: 'Merge Collections' }));

    await waitFor(() => expect(organizeIntoCollection).toHaveBeenCalledWith({
      target,
      label: null,
      winning_collection_id: 8,
    }));
  });
});

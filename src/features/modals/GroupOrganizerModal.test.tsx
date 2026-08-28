import { fireEvent, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { GroupOrganizerModal } from './GroupOrganizerModal';

const organizeIntoGroup = vi.hoisted(() => vi.fn());
vi.mock('../../platform/entityApi', () => ({ organizeIntoGroup }));

const target = { kind: 'explicit' as const, root_ids: [1, 2] };

beforeEach(() => {
  organizeIntoGroup.mockReset();
  organizeIntoGroup.mockResolvedValue({ collection_id: 9, receipt: {} });
});

describe('GroupOrganizerModal', () => {
  it('requires a name when creating a group from standalone items', async () => {
    const onComplete = vi.fn();
    const onClose = vi.fn();
    renderWithProviders(
      <GroupOrganizerModal
        open
        target={target}
        coverRootId={1}
        groups={[]}
        onClose={onClose}
        onComplete={onComplete}
      />,
    );

    fireEvent.change(await screen.findByPlaceholderText('Group name'), {
      target: { value: 'Chapter one' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Create Group' }));

    await waitFor(() => expect(organizeIntoGroup).toHaveBeenCalledWith({
      target,
      cover_root_id: 1,
      name: 'Chapter one',
      winning_collection_id: null,
    }));
    expect(onComplete).toHaveBeenCalledWith(9);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('requires the user to choose the winning group when several are selected', async () => {
    renderWithProviders(
      <GroupOrganizerModal
        open
        target={target}
        coverRootId={1}
        groups={[
          { collection_id: 7, label: 'Left', member_count: 2 },
          { collection_id: 8, label: 'Right', member_count: 4 },
        ]}
        onClose={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByText('Right'));
    fireEvent.click(screen.getByRole('button', { name: 'Merge Groups' }));

    await waitFor(() => expect(organizeIntoGroup).toHaveBeenCalledWith({
      target,
      cover_root_id: 1,
      name: null,
      winning_collection_id: 8,
    }));
  });
});

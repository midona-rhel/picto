import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { SubscriptionCoverCandidate } from '../../../shared/types/generated/application/SubscriptionCoverCandidate';
import { SubscriptionCoverDialog } from './SubscriptionCoverDialog';

const candidate: SubscriptionCoverCandidate = {
  media_item_id: 42,
  file_hash: 'cover-hash',
  name: 'Cover candidate',
  pixel_width: 1200,
  pixel_height: 800,
};

describe('SubscriptionCoverDialog', () => {
  it('uses thumbnails in the candidate grid and the full file only in the crop step', async () => {
    render(
      <SubscriptionCoverDialog
        target={{ id: '7', name: 'Artist' }}
        busy={false}
        onLoad={vi.fn().mockResolvedValue({ candidates: [candidate], next_cursor: null })}
        onSave={vi.fn().mockResolvedValue(true)}
        onClose={vi.fn()}
      />,
    );

    const candidateButton = await screen.findByTitle('Cover candidate');
    expect(candidateButton.querySelector('img')).toHaveAttribute(
      'src',
      'media://localhost/thumb/cover-hash.jpg',
    );
    fireEvent.click(candidateButton);
    expect(screen.getByLabelText('Cover crop preview').querySelector('img')).toHaveAttribute(
      'src',
      'media://localhost/file/cover-hash.bin',
    );
  });

  it('keeps large candidate pages virtualized instead of mounting every image', async () => {
    const candidates = Array.from({ length: 200 }, (_, index) => ({
      ...candidate,
      media_item_id: index + 1,
      file_hash: `cover-${index + 1}`,
      name: `Candidate ${index + 1}`,
    }));
    render(
      <SubscriptionCoverDialog
        target={{ id: '7', name: 'Artist' }}
        busy={false}
        onLoad={vi.fn().mockResolvedValue({ candidates, next_cursor: null })}
        onSave={vi.fn().mockResolvedValue(true)}
        onClose={vi.fn()}
      />,
    );

    await screen.findByTitle('Candidate 1');
    const mountedCandidates = screen.getAllByRole('button', { name: /Candidate/ });
    expect(mountedCandidates.length).toBeGreaterThan(0);
    expect(mountedCandidates.length).toBeLessThan(candidates.length);
  });

  it('keeps the selected crop open across parent refreshes', async () => {
    const firstLoad = vi.fn().mockResolvedValue({ candidates: [candidate], next_cursor: null });
    const onSave = vi.fn().mockResolvedValue(true);
    const target = { id: '7', name: 'Artist' };
    const view = render(
      <SubscriptionCoverDialog
        target={target}
        busy={false}
        onLoad={firstLoad}
        onSave={onSave}
        onClose={vi.fn()}
      />,
    );

    const candidateButton = await screen.findByTitle('Cover candidate');
    fireEvent.click(candidateButton);
    expect(screen.getByText('Zoom')).toBeInTheDocument();

    const refreshedLoad = vi.fn().mockResolvedValue({ candidates: [candidate], next_cursor: null });
    view.rerender(
      <SubscriptionCoverDialog
        target={target}
        busy={false}
        onLoad={refreshedLoad}
        onSave={onSave}
        onClose={vi.fn()}
      />,
    );

    await waitFor(() => expect(screen.getByText('Zoom')).toBeInTheDocument());
    expect(refreshedLoad).not.toHaveBeenCalled();
  });

  it('zooms the crop with the mouse wheel and clamps the saved value', async () => {
    const onSave = vi.fn().mockResolvedValue(true);
    render(
      <SubscriptionCoverDialog
        target={{ id: '7', name: 'Artist' }}
        busy={false}
        onLoad={vi.fn().mockResolvedValue({ candidates: [candidate], next_cursor: null })}
        onSave={onSave}
        onClose={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByTitle('Cover candidate'));
    const preview = screen.getByLabelText('Cover crop preview');
    const zoom = screen.getByLabelText('Cover zoom');
    expect(preview.querySelector('img')).toHaveAttribute(
      'src',
      'media://localhost/file/cover-hash.bin',
    );
    expect(zoom).toHaveValue('100');

    fireEvent.wheel(preview, { deltaY: -120 });
    expect(zoom).toHaveValue('105');
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

    await waitFor(() => expect(onSave).toHaveBeenCalledWith(
      '7',
      42,
      expect.objectContaining({ zoomPercent: 105 }),
    ));
  });

  it('moves a zoomed landscape cover vertically and horizontally', async () => {
    const onSave = vi.fn().mockResolvedValue(true);
    render(
      <SubscriptionCoverDialog
        target={{ id: '7', name: 'Artist' }}
        busy={false}
        onLoad={vi.fn().mockResolvedValue({ candidates: [candidate], next_cursor: null })}
        onSave={onSave}
        onClose={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByTitle('Cover candidate'));
    const preview = screen.getByLabelText('Cover crop preview');
    Object.defineProperty(preview, 'getBoundingClientRect', {
      value: () => ({ width: 400, height: 400, left: 0, top: 0, right: 400, bottom: 400 }),
    });
    Object.defineProperty(preview, 'setPointerCapture', { value: vi.fn() });
    fireEvent.change(screen.getByLabelText('Cover zoom'), { target: { value: '200' } });
    fireEvent(preview, new MouseEvent('pointerdown', { bubbles: true, clientX: 200, clientY: 200 }));
    fireEvent(preview, new MouseEvent('pointermove', { bubbles: true, clientX: 240, clientY: 240 }));
    fireEvent(preview, new MouseEvent('pointerup', { bubbles: true, clientX: 240, clientY: 240 }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

    await waitFor(() => expect(onSave).toHaveBeenCalledWith(
      '7',
      42,
      expect.objectContaining({ focusX: expect.any(Number), focusY: expect.any(Number) }),
    ));
    const savedCrop = onSave.mock.calls[0]?.[2];
    expect(savedCrop.focusX).toBeLessThan(500);
    expect(savedCrop.focusY).toBeLessThan(500);
  });
});

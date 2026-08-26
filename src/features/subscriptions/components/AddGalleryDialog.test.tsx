import { fireEvent, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../../test/render';
import { AddGalleryDialog } from './AddGalleryDialog';

describe('AddGalleryDialog', () => {
  it('submits a concrete gallery for the selected supported provider', () => {
    const onAdd = vi.fn();
    renderWithProviders(
      <AddGalleryDialog open busy={false} onAdd={onAdd} onClose={vi.fn()} />,
    );

    expect(screen.getByRole('button', { name: 'Gallery service' })).toHaveTextContent('E-Hentai');
    fireEvent.change(screen.getByLabelText('Gallery URL'), {
      target: { value: '  https://e-hentai.org/g/12345/67890abcde/  ' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add Gallery' }));

    expect(onAdd).toHaveBeenCalledWith({
      serviceId: 'ehentai',
      url: 'https://e-hentai.org/g/12345/67890abcde/',
    });

    fireEvent.click(screen.getByRole('button', { name: 'Gallery service' }));
    fireEvent.click(screen.getByRole('option', { name: 'ExHentai' }));
    expect(screen.getByLabelText('Gallery URL')).toHaveAttribute(
      'placeholder',
      'https://exhentai.org/g/12345/67890abcde/',
    );
  });
});

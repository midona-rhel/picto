import { screen } from '@testing-library/react';
import { expect, test, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { PictoPackModal } from './PictoPackModal';

vi.mock('../../platform/pictoPackApi', async (original) => ({
  ...await original<typeof import('../../platform/pictoPackApi')>(),
  exportPictoPack: vi.fn(),
  importPictoPack: vi.fn(),
}));

test('previews an inspected pack before importing it', () => {
  renderWithProviders(
    <PictoPackModal
      state={{
        open: true,
        mode: 'import',
        path: '/tmp/portfolio.picto-pack',
        summary: {
          name: 'Portfolio',
          source_kind: 'folder',
          root_count: 12,
          media_count: 18,
          folder_count: 3,
          smart_folder_count: 0,
          total_bytes: 2048,
        },
      }}
      onClose={vi.fn()}
    />,
  );

  expect(screen.getByRole('dialog', { name: 'Import Picto Pack' })).toBeVisible();
  expect(screen.getByText('Portfolio')).toBeVisible();
  expect(screen.getByText('Subscriptions, provider history, authentication, and folder watch paths are never included.')).toBeVisible();
  expect(screen.getByText('2.00 KB')).toBeVisible();
});

test('explains that a smart-folder export is an item snapshot without rules', () => {
  renderWithProviders(
    <PictoPackModal
      state={{
        open: true,
        mode: 'export',
        source: { kind: 'smart_folder', smart_folder_id: 9 },
        itemCount: 25,
        suggestedName: 'Recent favorites',
      }}
      onClose={vi.fn()}
    />,
  );

  expect(screen.getByText('Smart-folder exports contain only the current matching items. The smart-folder rule and container are not included.')).toBeVisible();
  expect(screen.getByText('25')).toBeVisible();
});

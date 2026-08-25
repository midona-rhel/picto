import { screen } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { InspectorSkeleton } from './Inspector';

const coreLabels = [
  'Items', 'Dimensions', 'Size', 'Type', 'Duration',
  'Date added', 'Date created', 'Date modified',
];

beforeAll(() => {
  class TestResizeObserver {
    observe() {}
    disconnect() {}
  }
  Object.defineProperty(globalThis, 'ResizeObserver', { value: TestResizeObserver });
});

function renderState(state: 'entity' | 'multi' | 'folder' | 'smart-folder' | 'system' | 'loading' | 'error') {
  const unavailable = state !== 'entity';
  renderWithProviders(
    <InspectorSkeleton
      preview={<div />}
      palette={[]}
      selectionCount={state === 'multi' ? 2 : undefined}
      name={state === 'multi' ? undefined : { value: state === 'loading' || state === 'error' ? '—' : 'Example' }}
      notes={{ value: state === 'entity' ? 'Notes' : '—', readOnly: unavailable }}
      source={{ urls: state === 'entity' || state === 'multi' ? ['https://example.com/item'] : [], unavailable: state !== 'entity' && state !== 'multi' }}
      showSource={!['folder', 'smart-folder', 'system'].includes(state)}
      rating={{ value: 0 }}
      coreProperties={coreLabels.map((label, index) => ({
        label: label as 'Items' | 'Dimensions' | 'Size' | 'Type' | 'Duration' | 'Date added' | 'Date created' | 'Date modified',
        value: index === 0 && state !== 'multi' ? '1' : '—',
        mono: label !== 'Type',
      }))}
      tags={[]}
      showTags={!['folder', 'smart-folder', 'system', 'loading', 'error'].includes(state)}
      folders={[]}
      showFolders={!['folder', 'smart-folder', 'system', 'loading', 'error'].includes(state)}
      status={state === 'loading' ? { kind: 'loading', message: 'Loading...' } : state === 'error' ? { kind: 'error', message: 'Unavailable' } : undefined}
    />,
  );
}

describe('InspectorSkeleton', () => {
  for (const state of ['entity', 'multi', 'folder', 'smart-folder', 'system', 'loading', 'error'] as const) {
    it(`keeps identity and core anchors for ${state}`, () => {
      renderState(state);

      const expectedAnchors = ['folder', 'smart-folder', 'system'].includes(state)
        ? ['name', 'notes']
        : state === 'multi' ? ['notes', 'source'] : ['name', 'notes', 'source'];
      expect([...document.querySelectorAll('[data-inspector-anchor]')].map((node) => node.getAttribute('data-inspector-anchor')))
        .toEqual(expectedAnchors);
      expect([...document.querySelectorAll('[data-inspector-core-property]')].map((node) => node.getAttribute('data-inspector-core-property')))
        .toEqual(state === 'multi' ? [] : ['Items']);
      if (state === 'multi') expect(screen.getByText('2 items selected')).toBeInTheDocument();
      expect(screen.getByText('Properties')).toBeInTheDocument();

      const applicable = state === 'entity' || state === 'multi';
      expect(Boolean(document.querySelector('[data-inspector-section="tags"]'))).toBe(applicable);
      expect(Boolean(document.querySelector('[data-inspector-section="folders"]'))).toBe(applicable);

      if (state === 'loading') expect(screen.getByRole('status')).toHaveTextContent('Loading...');
      if (state === 'error') expect(screen.getByRole('status')).toHaveTextContent('Unavailable');
    });
  }
});

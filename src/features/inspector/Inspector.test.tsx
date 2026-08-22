import { render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
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
  render(
    <InspectorSkeleton
      preview={<div />}
      palette={[]}
      name={{ value: state === 'multi' ? '2 items selected' : state === 'loading' || state === 'error' ? '—' : 'Example' }}
      notes={{ value: state === 'entity' ? 'Notes' : '—', readOnly: unavailable }}
      source={{ urls: state === 'entity' ? ['https://example.com/item'] : [], unavailable }}
      rating={{ value: 0 }}
      coreProperties={coreLabels.map((label, index) => ({
        label: label as 'Items' | 'Dimensions' | 'Size' | 'Type' | 'Duration' | 'Date added' | 'Date created' | 'Date modified',
        value: index === 0 ? '1' : '—',
        mono: label !== 'Type',
      }))}
      tags={[]}
      folders={[]}
      status={state === 'loading' ? { kind: 'loading', message: 'Loading...' } : state === 'error' ? { kind: 'error', message: 'Unavailable' } : undefined}
    />,
  );
}

describe('InspectorSkeleton', () => {
  for (const state of ['entity', 'multi', 'folder', 'smart-folder', 'system', 'loading', 'error'] as const) {
    it(`keeps identity and core anchors for ${state}`, () => {
      renderState(state);

      expect(document.querySelectorAll('[data-inspector-anchor]')).toHaveLength(3);
      expect([...document.querySelectorAll('[data-inspector-anchor]')].map((node) => node.getAttribute('data-inspector-anchor')))
        .toEqual(['name', 'notes', 'source']);
      expect([...document.querySelectorAll('[data-inspector-core-property]')].map((node) => node.getAttribute('data-inspector-core-property')))
        .toEqual(coreLabels);
      expect(screen.getByText('Properties')).toBeInTheDocument();

      if (state === 'loading') expect(screen.getByRole('status')).toHaveTextContent('Loading...');
      if (state === 'error') expect(screen.getByRole('status')).toHaveTextContent('Unavailable');
    });
  }
});

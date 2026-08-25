import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FontViewer } from './FontViewer';

class MockFontFace {
  constructor(public family: string) {}
  load() { return Promise.resolve(this as unknown as FontFace); }
}

describe('FontViewer', () => {
  const add = vi.fn();
  const remove = vi.fn();

  beforeEach(() => {
    vi.stubGlobal('FontFace', MockFontFace);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, arrayBuffer: () => Promise.resolve(new ArrayBuffer(8)) }));
    Object.defineProperty(document, 'fonts', { configurable: true, value: { add, delete: remove } });
  });

  afterEach(() => { vi.unstubAllGlobals(); });

  it('loads the font and exposes preview modes and themes', async () => {
    render(<FontViewer src="media://localhost/file/hash.ttf" displayName="Example.ttf" mimeType="font/ttf" />);
    await waitFor(() => expect(add).toHaveBeenCalled());

    expect(screen.getByRole('heading', { name: 'Example.ttf' })).toBeInTheDocument();
    expect(screen.getAllByRole('tab').map((tab) => tab.textContent)).toEqual(['Preview', 'Waterfall', 'Glyphs', 'Information']);
    fireEvent.click(screen.getByRole('button', { name: 'purple preview' }));
    expect(screen.getByRole('button', { name: 'purple preview' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('keeps one zoom control across preview tabs and hides it for information', async () => {
    render(<FontViewer src="media://localhost/file/hash.otf" displayName="Example.otf" mimeType="font/otf" />);
    await waitFor(() => expect(add).toHaveBeenCalled());
    expect(screen.getByRole('slider', { name: 'Font preview size' })).toHaveValue('100');
    fireEvent.click(screen.getByRole('button', { name: 'Increase font preview size' }));
    expect(screen.getByRole('slider', { name: 'Font preview size' })).toHaveValue('110');
    fireEvent.click(screen.getByRole('tab', { name: 'Information' }));
    expect(screen.queryByRole('slider', { name: 'Font preview size' })).not.toBeInTheDocument();
  });
});

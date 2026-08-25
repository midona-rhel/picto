import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import JSZip from 'jszip';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ArchiveDocumentViewer } from './ArchiveDocumentViewer';

async function cbzBytes() {
  const zip = new JSZip();
  zip.file('10.png', new Uint8Array([10]));
  zip.file('2.png', new Uint8Array([2]));
  return zip.generateAsync({ type: 'arraybuffer' });
}

async function epubBytes() {
  const zip = new JSZip();
  zip.file('META-INF/container.xml', '<?xml version="1.0"?><container><rootfiles><rootfile full-path="OPS/book.opf" /></rootfiles></container>');
  zip.file('OPS/book.opf', '<package><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>');
  zip.file('OPS/one.xhtml', '<html><body><h1>First chapter</h1></body></html>');
  zip.file('OPS/two.xhtml', '<html><body><h1>Second chapter</h1></body></html>');
  return zip.generateAsync({ type: 'arraybuffer' });
}

function renderViewer(kind: 'epub' | 'cbz') {
  return render(<MantineProvider><ArchiveDocumentViewer src={`media://book.${kind}`} kind={kind} /></MantineProvider>);
}

describe('ArchiveDocumentViewer', () => {
  const originalCreateObjectURL = URL.createObjectURL;
  const originalRevokeObjectURL = URL.revokeObjectURL;

  afterEach(() => {
    URL.createObjectURL = originalCreateObjectURL;
    URL.revokeObjectURL = originalRevokeObjectURL;
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('natural-sorts CBZ pages and uses the shared document footer', async () => {
    const bytes = await cbzBytes();
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, arrayBuffer: () => Promise.resolve(bytes) }));
    URL.createObjectURL = vi.fn(() => 'blob:page');
    URL.revokeObjectURL = vi.fn();
    renderViewer('cbz');
    await waitFor(() => expect(screen.getByText('Page 1 of 2')).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: 'Next comic page' }));
    expect(screen.getByText('Page 2 of 2')).toBeInTheDocument();
  });

  it('reads EPUB spine order and renders sanitized chapter markup', async () => {
    const bytes = await epubBytes();
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, arrayBuffer: () => Promise.resolve(bytes) }));
    renderViewer('epub');
    await waitFor(() => expect(screen.getByText('First chapter')).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: 'Next book page' }));
    await waitFor(() => expect(screen.getByText('Second chapter')).toBeInTheDocument());
  });
});

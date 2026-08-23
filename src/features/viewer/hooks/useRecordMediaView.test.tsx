import { render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

const { recordMediaView } = vi.hoisted(() => ({
  recordMediaView: vi.fn(() => Promise.resolve()),
}));

vi.mock('../../../controllers/viewerController', () => ({
  viewerController: { recordMediaView },
}));

import { useRecordMediaView } from './useRecordMediaView';

function Viewer({ itemId }: { itemId: number | null }) {
  useRecordMediaView(itemId);
  return null;
}

describe('useRecordMediaView', () => {
  it('records each logical entity as the viewer changes', () => {
    const view = render(<Viewer itemId={1} />);
    view.rerender(<Viewer itemId={1} />);
    view.rerender(<Viewer itemId={2} />);
    view.rerender(<Viewer itemId={null} />);

    expect(recordMediaView).toHaveBeenCalledTimes(2);
    expect(recordMediaView).toHaveBeenNthCalledWith(1, 1);
    expect(recordMediaView).toHaveBeenNthCalledWith(2, 2);
  });
});

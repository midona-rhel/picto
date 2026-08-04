import { render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

const { recordMediaView } = vi.hoisted(() => ({
  recordMediaView: vi.fn(() => Promise.resolve()),
}));

vi.mock('../../../controllers/viewerController', () => ({
  viewerController: { recordMediaView },
}));

import { useRecordMediaView } from './useRecordMediaView';

function Viewer({ entityHash }: { entityHash: string | null }) {
  useRecordMediaView(entityHash);
  return null;
}

describe('useRecordMediaView', () => {
  it('records each logical entity as the viewer changes', () => {
    const view = render(<Viewer entityHash="one" />);
    view.rerender(<Viewer entityHash="one" />);
    view.rerender(<Viewer entityHash="two" />);
    view.rerender(<Viewer entityHash={null} />);

    expect(recordMediaView).toHaveBeenCalledTimes(2);
    expect(recordMediaView).toHaveBeenNthCalledWith(1, 'one');
    expect(recordMediaView).toHaveBeenNthCalledWith(2, 'two');
  });
});

import { beforeEach, expect, test, vi } from 'vitest';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('./ipc', () => ({ invoke }));

import { exportPictoPack, importPictoPack, inspectPictoPack } from './pictoPackApi';

beforeEach(() => invoke.mockReset());

test('inspects a pack before import', async () => {
  invoke.mockResolvedValue({ root_count: 2 });
  await expect(inspectPictoPack('/tmp/example.picto-pack')).resolves.toEqual({ root_count: 2 });
  expect(invoke).toHaveBeenCalledWith('picto_pack.inspect', { path: '/tmp/example.picto-pack' });
});

test('exports the exact portable source and destination', async () => {
  invoke.mockResolvedValue({ output_path: '/tmp/example.picto-pack' });
  const source = { kind: 'items' as const, target: { kind: 'explicit' as const, root_ids: [4, 7] } };
  await exportPictoPack(source, '/tmp/example.picto-pack');
  expect(invoke).toHaveBeenCalledWith('picto_pack.export', {
    source,
    output_path: '/tmp/example.picto-pack',
  });
});

test('imports only after the user confirms the inspected pack', async () => {
  invoke.mockResolvedValue({ imported_roots: 2 });
  await importPictoPack('/tmp/example.picto-pack');
  expect(invoke).toHaveBeenCalledWith('picto_pack.import', { path: '/tmp/example.picto-pack' });
});

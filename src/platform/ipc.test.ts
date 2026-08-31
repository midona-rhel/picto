import { invoke, normalizeInvokeError } from './ipc';

afterEach(() => {
  delete (window as any).picto;
});

describe('normalizeInvokeError', () => {
  it('removes Electron transport wrappers from backend messages', () => {
    const error = new Error(
      "Error invoking remote method 'picto:invoke': Error: This subscription is already running.",
    );

    expect(normalizeInvokeError(error).message).toBe('This subscription is already running.');
  });

  it('normalizes dedicated Electron channels as well as the generic invoke channel', () => {
    const error = new Error(
      "Error invoking remote method 'picto:library:joinCloud': Error: The selected destination already exists.",
    );

    expect(normalizeInvokeError(error).message).toBe('The selected destination already exists.');
  });

  it('keeps direct product messages intact', () => {
    expect(normalizeInvokeError(new Error('Open a library first.')).message).toBe(
      'Open a library first.',
    );
  });

  it('decodes the native JSON envelope in the renderer', async () => {
    (window as any).picto = {
      api: {
        invoke: vi.fn().mockResolvedValue({
          __pictoCoreJson: '{"items":[{"item_id":7}],"revision":2}',
          __pictoNativeMs: 0.4,
        }),
      },
    };

    await expect(invoke('items.query', {})).resolves.toEqual({
      items: [{ item_id: 7 }],
      revision: 2,
    });
  });
});

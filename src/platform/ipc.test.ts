import { normalizeInvokeError } from './ipc';

describe('normalizeInvokeError', () => {
  it('removes Electron transport wrappers from backend messages', () => {
    const error = new Error(
      "Error invoking remote method 'picto:invoke': Error: This subscription is already running.",
    );

    expect(normalizeInvokeError(error).message).toBe('This subscription is already running.');
  });

  it('keeps direct product messages intact', () => {
    expect(normalizeInvokeError(new Error('Open a library first.')).message).toBe(
      'Open a library first.',
    );
  });
});

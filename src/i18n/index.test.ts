import { applyDocumentLocale, getLocale, setLocale, startLocalizedRenderer, t, translateMessage } from './index';

describe('localization runtime', () => {
  beforeEach(() => localStorage.clear());

  it('uses the persisted locale and translates catalog messages', () => {
    localStorage.setItem('picto:locale', 'de');
    expect(getLocale()).toBe('de');
    expect(t('Settings')).toBe('Einstellungen');
  });

  it('falls back to English for unknown locale values and messages', () => {
    localStorage.setItem('picto:locale', 'unknown');
    expect(getLocale()).toBe('en');
    expect(t('Uncatalogued test message')).toBe('Uncatalogued test message');
  });

  it('preserves and interpolates catalog placeholders', () => {
    localStorage.setItem('picto:locale', 'es');
    const result = t('Review item {value0}', { value0: 4 });
    expect(result).toContain('4');
    expect(result).not.toContain('{value0}');
  });

  it('translates rename actions in Spanish', () => {
    localStorage.setItem('picto:locale', 'es');
    expect(t('Rename')).toBe('Renombrar');
    expect(t('Rename Group…')).toBe('Renombrar grupo…');
    expect(t('Rename selected file')).toBe('Renombrar el archivo seleccionado');
  });

  it('sets the document language', () => {
    applyDocumentLocale('fi');
    expect(document.documentElement.lang).toBe('fi');
  });

  it('updates an existing renderer once without reloading the document', () => {
    const render = vi.fn();
    const stop = startLocalizedRenderer(render);

    setLocale('de');

    expect(render).toHaveBeenCalledTimes(2);
    expect(document.documentElement.lang).toBe('de');
    expect(t('Settings')).toBe('Einstellungen');
    stop();
  });

  it('retranslates labels cached while a previous locale was active', () => {
    localStorage.setItem('picto:locale', 'de');
    const cachedLabel = t('Settings');
    expect(cachedLabel).toBe('Einstellungen');

    setLocale('en');

    expect(translateMessage(cachedLabel)).toBe('Settings');
  });
});

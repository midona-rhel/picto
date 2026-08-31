import de from './locales/de.json';
import en from './locales/en.json';
import es from './locales/es.json';
import fi from './locales/fi.json';
import fr from './locales/fr.json';
import ja from './locales/ja.json';
import pt from './locales/pt.json';
import zhCn from './locales/zh-CN.json';

export const LOCALES = ['en', 'de', 'es', 'pt', 'fr', 'zh-CN', 'ja', 'fi'] as const;
export type Locale = typeof LOCALES[number];

export const LOCALE_OPTIONS: Array<{ value: Locale; label: string }> = [
  { value: 'en', label: 'English' },
  { value: 'de', label: 'Deutsch' },
  { value: 'es', label: 'Español' },
  { value: 'pt', label: 'Português' },
  { value: 'fr', label: 'Français' },
  { value: 'zh-CN', label: '简体中文' },
  { value: 'ja', label: '日本語' },
  { value: 'fi', label: 'Suomi' },
];

type Catalog = Record<string, string>;
type InterpolationValues = Record<string, string | number>;

const STORAGE_KEY = 'picto:locale';
const catalogs: Record<Locale, Catalog> = { de, en, es, fi, fr, ja, pt, 'zh-CN': zhCn };
const localeListeners = new Set<() => void>();
const localeChannels = new Set<BroadcastChannel>();
const messageKeyByTranslation = new Map<string, string>();
for (const catalog of Object.values(catalogs)) {
  for (const [message, translation] of Object.entries(catalog)) {
    if (!messageKeyByTranslation.has(translation)) messageKeyByTranslation.set(translation, message);
  }
}

function normalizeLocale(value: string | null | undefined): Locale | null {
  if (!value) return null;
  const normalized = value.toLowerCase();
  if (normalized.startsWith('de')) return 'de';
  if (normalized.startsWith('es')) return 'es';
  if (normalized.startsWith('pt')) return 'pt';
  if (normalized.startsWith('fr')) return 'fr';
  if (normalized.startsWith('zh')) return 'zh-CN';
  if (normalized.startsWith('ja')) return 'ja';
  if (normalized.startsWith('fi')) return 'fi';
  if (normalized.startsWith('en')) return 'en';
  return null;
}

export function getLocale(): Locale {
  const stored = typeof localStorage === 'undefined' ? null : normalizeLocale(localStorage.getItem(STORAGE_KEY));
  return stored ?? normalizeLocale(typeof navigator === 'undefined' ? null : navigator.language) ?? 'en';
}

/** Translate a message key held in runtime data or a module-level registry. */
export function translateMessage(message: string, values?: InterpolationValues): string {
  // Static option registries may have been initialized before a locale switch.
  // Recover their English message key so rendering can translate them again.
  const messageKey = catalogs.en[message] == null
    ? messageKeyByTranslation.get(message) ?? message
    : message;
  const template = catalogs[getLocale()][messageKey] ?? catalogs.en[messageKey] ?? messageKey;
  if (!values) return template;
  return template.replace(/\{([A-Za-z][A-Za-z0-9_]*)\}/g, (token, name: string) =>
    Object.prototype.hasOwnProperty.call(values, name) ? String(values[name]) : token,
  );
}

/** Translate a literal message. The localization gate extracts these calls. */
export function t(message: string, values?: InterpolationValues): string {
  return translateMessage(message, values);
}

export function setLocale(locale: Locale): void {
  if (typeof localStorage === 'undefined') return;
  if (getLocale() === locale) return;
  localStorage.setItem(STORAGE_KEY, locale);
  applyDocumentLocale(locale);
  localeListeners.forEach((listener) => listener());
  localeChannels.forEach((channel) => channel.postMessage(locale));
}

export function applyDocumentLocale(locale = getLocale()): void {
  document.documentElement.lang = locale;
}

export function startLocalizationRuntime(): () => void {
  applyDocumentLocale();
  const channel = typeof BroadcastChannel === 'undefined'
    ? null
    : new BroadcastChannel('picto:locale');
  if (channel) localeChannels.add(channel);

  const applyExternalLocale = (locale: Locale) => {
    // Electron windows have separate JavaScript contexts. Persist the value in
    // the receiving context as well, then reconcile its existing React tree.
    localStorage.setItem(STORAGE_KEY, locale);
    applyDocumentLocale(locale);
    localeListeners.forEach((listener) => listener());
  };
  const handleStorage = (event: StorageEvent) => {
    const locale = normalizeLocale(event.newValue);
    if (event.key === STORAGE_KEY && locale && document.documentElement.lang !== locale) {
      applyExternalLocale(locale);
    }
  };
  const handleBroadcast = (event: MessageEvent<unknown>) => {
    const locale = normalizeLocale(typeof event.data === 'string' ? event.data : null);
    if (locale && document.documentElement.lang !== locale) applyExternalLocale(locale);
  };
  window.addEventListener('storage', handleStorage);
  channel?.addEventListener('message', handleBroadcast);
  return () => {
    window.removeEventListener('storage', handleStorage);
    channel?.removeEventListener('message', handleBroadcast);
    if (channel) localeChannels.delete(channel);
    channel?.close();
  };
}

/** Reconcile the existing React tree when translations change, preserving all UI state. */
export function startLocalizedRenderer(render: () => void): () => void {
  const stopRuntime = startLocalizationRuntime();
  localeListeners.add(render);
  render();
  return () => {
    localeListeners.delete(render);
    stopRuntime();
  };
}
